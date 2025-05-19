/// Representation of a NAF encoding. The encoding is done in the following way:
/// - Value 1 is encoded as 1 inside the array,
/// - Value 0 is encoded as 0 inside the array,
/// - Value -1 is encoded as 2 inside the array.
pub struct NafEncoding(Vec<u8>);

impl NafEncoding {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn new(capacity: usize) -> Self {
        Self(vec![0; capacity])
    }

    pub fn create_neg(&mut self, idx: usize) {
        self.0[idx] = 2;
    }

    pub fn create_pos(&mut self, idx: usize) {
        self.0[idx] = 1;
    }

    pub fn create_zero(&mut self, idx: usize) {
        self.0[idx] = 0;
    }

    pub fn pos(&self, idx: usize) -> bool {
        self.0[idx] == 1
    }

    pub fn neg(&self, idx: usize) -> bool {
        self.0[idx] == 2
    }

    pub fn zero(&self, idx: usize) -> bool {
        self.0[idx] == 0
    }
}
