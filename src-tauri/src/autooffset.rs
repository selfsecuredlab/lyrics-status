//! Port of src/Autooffset.ts — a short ring of recent Discord round-trip times
//! whose average feeds the lyric send-ahead offset. The slightly odd padding in
//! `set_limit` is preserved from the original on purpose.

pub struct Autooffset {
    keys: Vec<i64>,
    pub limit: i64,
}

impl Autooffset {
    pub fn new() -> Autooffset {
        Autooffset {
            keys: Vec::new(),
            limit: 0,
        }
    }

    /// Drop the oldest (tail) value and push the newest to the front.
    pub fn add_value(&mut self, value: i64) {
        self.keys.pop();
        self.keys.insert(0, value);
    }

    pub fn average(&self) -> f64 {
        if self.keys.is_empty() {
            return f64::NAN; // matches JS 0/0
        }
        let sum: i64 = self.keys.iter().sum();
        sum as f64 / self.keys.len() as f64
    }

    pub fn set_limit(&mut self, limit: i64) {
        self.limit = limit;
        let limit_usize = limit.max(0) as usize;
        if self.keys.len() > limit_usize {
            self.keys.truncate(limit_usize);
        }
        let mut i = limit;
        while i > self.keys.len() as i64 {
            self.keys.push(0);
            i -= 1;
        }
    }
}
