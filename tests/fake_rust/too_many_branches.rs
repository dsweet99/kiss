//! A function with too many branches (threshold: 8)

pub fn branch_heavy(code: i32) -> &'static str {
    if code == 1 {
        "one"
    } else if code == 2 {
        "two"
    } else if code == 3 {
        "three"
    } else if code == 4 {
        "four"
    } else if code == 5 {
        "five"
    } else if code == 6 {
        "six"
    } else if code == 7 {
        "seven"
    } else if code == 8 {
        "eight"
    } else if code == 9 {
        "nine"
    } else if code == 10 {
        "ten"
    } else {
        "unknown"
    }
}

pub fn another_branch_fest(x: i32, y: i32) -> i32 {
    let mut result = 0;
    if x > 0 {
        result += 1;
    }
    if x > 10 {
        result += 2;
    }
    if x > 20 {
        result += 3;
    }
    if y > 0 {
        result += 4;
    }
    if y > 10 {
        result += 5;
    }
    if y > 20 {
        result += 6;
    }
    if x + y > 50 {
        result += 7;
    }
    if x * y > 100 {
        result += 8;
    }
    if x - y > 0 {
        result += 9;
    }
    result
}

