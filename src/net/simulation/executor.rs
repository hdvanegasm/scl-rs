//! Single-threaded cooperative executor that drives every party's task to completion.
//!
//! `run_simulation_with_idle` round-robins the party futures over a ready queue, re-polling a task only when
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
pub(crate) enum IdleOutcome {
    /// Nothing left to do but there are tasks still parked.
    Deadlocked,
    /// Progress was made.
    Progressed,
}

/// [`Wake`] implementation for a single task: waking it re-enqueues that task for polling.
///
/// One `TaskWaker` is built per task and handed to its future through the [`Context`] it is polled
/// with. When the future calls `wake`/`wake_by_ref` — typically because an awaited network receive
/// became ready — the waker pushes the task's [`id`](TaskWaker::id) onto the shared
/// [`ready_queue`](TaskWaker::ready_queue), so the executor loop picks the task up and polls it
/// again. A task that never wakes stays parked until `on_idle` delivers an event that wakes it.
struct TaskWaker {
    /// Index of the task this waker belongs to; also its slot in the executor's task list and the
    /// value pushed onto the ready queue.
    id: usize,
    /// The executor's shared queue of task ids awaiting a poll; waking pushes `id` onto it.
    ready_queue: Arc<Mutex<VecDeque<usize>>>,
}

impl Wake for TaskWaker {
    /// Wakes the task through the shared reference path; see [`wake_by_ref`](Self::wake_by_ref).
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    /// Marks the task ready by pushing its `id` onto the shared ready queue, where the executor
    /// loop will find it and poll the task again.
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready_queue
            .lock()
            .expect("wake queue poisoned")
            .push_back(self.id);
    }
}

/// Drives every future in `tasks` to completion on a single-threaded, cooperative scheduler,
/// invoking `on_idle` to make external progress whenever all tasks are parked.
///
/// Tasks are polled round-robin through a ready queue and re-polled only when their waker fires:
/// each task's [`Waker`] pushes its id back onto the queue, so a task that returns
/// [`Poll::Pending`] is not polled again until something wakes it. When the ready queue drains
/// while tasks are still parked, `on_idle` is called to unblock them — in the simulator it
/// delivers the next scheduled network event in virtual-time order and reports back via
/// [`IdleOutcome`].
/// Returns once every task has completed.
///
/// # Panics
///
/// Panics if `on_idle` returns [`IdleOutcome::Deadlocked`] while tasks remain parked: no task can make
/// progress and nothing is left to deliver, so the protocol is genuinely stuck and the run is
/// aborted rather than left to hang.
pub(crate) fn run_simulation_with_idle<F>(
    tasks: Vec<Pin<Box<dyn Future<Output = ()>>>>,
    mut on_idle: F,
) where
    F: FnMut() -> IdleOutcome,
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

    // We include every task first for the first poll. All tasks are ready to make progress.
    {
        let mut ready_queue_guard = ready_queue.lock().expect("the ready queue is poisoned");
        for task_id in 0..tasks.len() {
            ready_queue_guard.push_back(task_id);
        }
    }

    let mut remaining_tasks = tasks.len();
    while remaining_tasks > 0 {
        let next_task = ready_queue
            .lock()
            .expect("the ready queue is poisoned")
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
            // There are no tasks in ready to make progress on them. Hence, we need to try to make
            // progress by processing pending delivery events and waking parked tasks.
            None => match on_idle() {
                // In this case there are remaining tasks, but all of them are parked.
                //
                // The `on_idle` function returns progressed when a new event is popped. When the
                // event queue is drained, we have hitted a problem: all tasks parked and no events to
                // continue mean a real deadlock.
                IdleOutcome::Deadlocked => {
                    panic!("scheduler: {remaining_tasks} task(s) parked, nothing to deliver (deadlock)");
                }
                IdleOutcome::Progressed => {}
            },
        }
    }
}
