/// Representation of a non-adjacent form (NAF) encoding.
///
/// The encoding is done in the following way:
/// - Value 1 is encoded as 1 inside the array,
/// - Value 0 is encoded as 0 inside the array,
/// - Value -1 is encoded as 2 inside the array.
#[derive(Eq, PartialEq, Debug)]
pub struct NafEncoding(Vec<u8>);

impl NafEncoding {
    /// Returns the length of the representation.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Checks if the current NAF encoding is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Creates a new NAF representation using a given maximum number of digits.
    pub fn new(capacity: usize) -> Self {
        Self(vec![0; capacity])
    }

    /// Sets the position `idx` to negative in the NAF encoding.
    pub fn create_neg(&mut self, idx: usize) {
        self.0[idx] = 2;
    }

    /// Sets the position `idx` to positive in the NAF encoding.
    pub fn create_pos(&mut self, idx: usize) {
        self.0[idx] = 1;
    }

    /// Sets the position `idx` to zero in the NAF encoding.
    pub fn create_zero(&mut self, idx: usize) {
        self.0[idx] = 0;
    }

    /// Returns `true` if the position `idx` in the encoding is positive.
    pub fn pos(&self, idx: usize) -> bool {
        self.0[idx] == 1
    }

    /// Returns `true` if the position `idx` in the encoding is negative.
    pub fn neg(&self, idx: usize) -> bool {
        self.0[idx] == 2
    }

    /// Returns `true` if the position `idx` in the encoding is zero.
    pub fn zero(&self, idx: usize) -> bool {
        self.0[idx] == 0
    }
}

impl From<Vec<u8>> for NafEncoding {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}
