//! A function with too many statements (threshold: 25)

pub fn bloated_function() {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    let e = 5;
    println!("{}", a);
    println!("{}", b);
    println!("{}", c);
    println!("{}", d);
    println!("{}", e);
    let f = 6;
    let g = 7;
    let h = 8;
    let i = 9;
    let j = 10;
    println!("{}", f);
    println!("{}", g);
    println!("{}", h);
    println!("{}", i);
    println!("{}", j);
    let k = 11;
    let l = 12;
    let m = 13;
    let n = 14;
    let o = 15;
    println!("{}", k + l + m + n + o);
}

