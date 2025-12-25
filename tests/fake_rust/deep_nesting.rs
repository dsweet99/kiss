//! Code with too much indentation depth (threshold: 4)

pub fn deeply_nested(x: i32) -> i32 {
    if x > 0 {
        if x > 10 {
            if x > 20 {
                if x > 30 {
                    if x > 40 {
                        // This is way too deep!
                        return x * 2;
                    }
                    return x + 40;
                }
                return x + 30;
            }
            return x + 20;
        }
        return x + 10;
    }
    0
}

pub fn nested_loops(data: Vec<Vec<Vec<i32>>>) -> i32 {
    let mut sum = 0;
    for layer1 in &data {
        for layer2 in layer1 {
            for layer3 in layer2 {
                if *layer3 > 0 {
                    if *layer3 < 100 {
                        sum += layer3;
                    }
                }
            }
        }
    }
    sum
}

