//! A type with too many methods (threshold: 15)

pub struct GodClass {
    data: Vec<i32>,
}

impl GodClass {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }
    
    pub fn method1(&self) -> i32 { 1 }
    pub fn method2(&self) -> i32 { 2 }
    pub fn method3(&self) -> i32 { 3 }
    pub fn method4(&self) -> i32 { 4 }
    pub fn method5(&self) -> i32 { 5 }
    pub fn method6(&self) -> i32 { 6 }
    pub fn method7(&self) -> i32 { 7 }
    pub fn method8(&self) -> i32 { 8 }
    pub fn method9(&self) -> i32 { 9 }
    pub fn method10(&self) -> i32 { 10 }
    pub fn method11(&self) -> i32 { 11 }
    pub fn method12(&self) -> i32 { 12 }
    pub fn method13(&self) -> i32 { 13 }
    pub fn method14(&self) -> i32 { 14 }
    pub fn method15(&self) -> i32 { 15 }
    pub fn method16(&self) -> i32 { 16 }
    pub fn method17(&self) -> i32 { 17 }
    pub fn method18(&self) -> i32 { 18 }
    pub fn method19(&self) -> i32 { 19 }
    pub fn method20(&self) -> i32 { 20 }
}

/// Another type with excessive methods
pub struct AnotherGodClass {
    value: String,
}

impl AnotherGodClass {
    pub fn a(&self) -> &str { &self.value }
    pub fn b(&self) -> &str { &self.value }
    pub fn c(&self) -> &str { &self.value }
    pub fn d(&self) -> &str { &self.value }
    pub fn e(&self) -> &str { &self.value }
    pub fn f(&self) -> &str { &self.value }
    pub fn g(&self) -> &str { &self.value }
    pub fn h(&self) -> &str { &self.value }
    pub fn i(&self) -> &str { &self.value }
    pub fn j(&self) -> &str { &self.value }
    pub fn k(&self) -> &str { &self.value }
    pub fn l(&self) -> &str { &self.value }
    pub fn m(&self) -> &str { &self.value }
    pub fn n(&self) -> &str { &self.value }
    pub fn o(&self) -> &str { &self.value }
    pub fn p(&self) -> &str { &self.value }
}

