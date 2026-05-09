pub mod dest;

pub struct Worker;

impl Worker {
    pub fn run(&self, value: i32) -> i32 {
        value + 1
    }
}

pub struct Reviewer;

impl Reviewer {
    pub fn run(&self, value: i32) -> i32 {
        value + 2
    }
}
