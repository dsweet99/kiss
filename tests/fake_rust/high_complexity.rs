//! A function with high cyclomatic complexity (threshold: 20)

pub fn complex_state_machine(state: i32, event: i32) -> i32 {
    let mut next_state = state;
    
    if state == 0 && event == 1 {
        next_state = 1;
    } else if state == 0 && event == 2 {
        next_state = 2;
    } else if state == 1 && event == 1 {
        next_state = 3;
    } else if state == 1 && event == 2 {
        next_state = 0;
    } else if state == 2 && event == 1 {
        next_state = 4;
    } else if state == 2 && event == 2 {
        next_state = 1;
    } else if state == 3 && event == 1 {
        next_state = 5;
    } else if state == 3 && event == 2 {
        next_state = 2;
    } else if state == 4 && event == 1 {
        next_state = 0;
    } else if state == 4 && event == 2 {
        next_state = 3;
    } else if state == 5 && event == 1 {
        next_state = 1;
    } else if state == 5 && event == 2 {
        next_state = 4;
    }
    
    // More conditions with boolean operators (adds to cyclomatic complexity)
    if (state > 0 && event > 0) || (state < 5 && event < 3) {
        next_state += 1;
    }
    
    if state == next_state && (event == 0 || event > 5) {
        next_state = 0;
    }
    
    if next_state > 10 || (next_state < 0 && state > 0) || event < 0 {
        next_state = state;
    }
    
    next_state
}

