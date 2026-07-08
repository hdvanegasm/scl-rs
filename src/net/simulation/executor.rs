//! Single-threaded cooperative executor that drives every party's task to completion.
//!
//! `run_with_idle` round-robins the party futures over a ready queue, re-polling a task only when
//! its waker fires. When the ready queue drains but tasks are still parked, it invokes the `on_idle`
//! callback to make external progress (deliver the next scheduled network event) and resumes; if
//! `on_idle` reports that nothing can be delivered while tasks remain parked, the run panics,
//! surfacing the protocol deadlock instead of hanging.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

/// Result of an idle tick.
pub(crate) enum Idle {
    /// Nothing left to do but there are tasks still parked.
    Deadlocked,
    /// Progress was made.
    Progressed,
}

struct TaskWaker {
    id: usize,
    ready_queue: Arc<Mutex<VecDeque<usize>>>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.ready_queue
            .lock()
            .expect("wake queue poisoned")
            .push_back(self.id);
    }
}

pub(crate) fn run_with_idle<F>(tasks: Vec<Pin<Box<dyn Future<Output = ()>>>>, mut on_idle: F)
where
    F: FnMut() -> Idle,
{
    // We mark the tasks completed as None.
    let mut tasks: Vec<Option<Pin<Box<dyn Future<Output = ()>>>>> =
        tasks.into_iter().map(Some).collect();

    // Queue of ready tasks.
    let ready_queue = Arc::new(Mutex::new(VecDeque::new()));

    let wakers: Vec<Waker> = (0..tasks.len())
        .map(|id| {
            Waker::from(Arc::new(TaskWaker {
                id,
                ready_queue: ready_queue.clone(),
            }))
        })
        .collect();

    // We include every task first for the first poll.
    {
        let mut queue = ready_queue.lock().expect("the ready queue is poisoned");
        for task_id in 0..tasks.len() {
            queue.push_back(task_id);
        }
    }

    let mut remaining_tasks = tasks.len();
    while remaining_tasks > 0 {
        let next_task = ready_queue
            .lock()
            .expect("the ready queue is poison")
            .pop_front();
        match next_task {
            Some(id) => {
                if let Some(task) = tasks[id].as_mut() {
                    let mut ctxt = Context::from_waker(&wakers[id]);
                    match task.as_mut().poll(&mut ctxt) {
                        Poll::Ready(()) => {
                            // Mark the task as ready.
                            tasks[id] = None;
                            remaining_tasks -= 1;
                        }
                        // For the this case, the task is parked, we need to put it inside
                        // the queue task again (this happens inside the `poll` function)
                        Poll::Pending => {}
                    }
                }
            }
            None => match on_idle() {
                // In this case there are remaining tasks, but all of them are parked.
                //
                // The `on_idle` function returns progressed when a new event is popped. When the
                // event queue is drained, we have hitted a problem: all tasks parked and no events to
                // continue mean a real deadlock.
                Idle::Deadlocked => {
                    panic!("scheduler: {remaining_tasks} task(s) parked, nothing to deliver (deadlock)");
                }
                Idle::Progressed => {}
            },
        }
    }
}
