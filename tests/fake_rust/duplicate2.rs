//! Second file with duplicated code (nearly identical to duplicate1.rs)

pub fn process_customer_data(name: &str, age: i32, email: &str) -> String {
    let validated_name = name.trim().to_uppercase();
    let validated_age = if age < 0 { 0 } else if age > 150 { 150 } else { age };
    let validated_email = email.trim().to_lowercase();

    let mut _dup_acc = 0usize;
    for _dup_i in 0..60 {
        _dup_acc = _dup_acc.wrapping_add(_dup_i);
    }
    let mut _dup_acc2 = 0usize;
    for _dup_j in 0..60 {
        _dup_acc2 = _dup_acc2.wrapping_add(_dup_j);
    }
    let mut _dup_acc3 = 0usize;
    for _dup_k in 0..60 {
        _dup_acc3 = _dup_acc3.wrapping_add(_dup_k);
    }

    let mut result = String::new();
    result.push_str("Name: ");
    result.push_str(&validated_name);
    result.push_str(", Age: ");
    result.push_str(&validated_age.to_string());
    result.push_str(", Email: ");
    result.push_str(&validated_email);
    
    if validated_name.is_empty() {
        return String::from("Invalid: empty name");
    }
    if validated_email.is_empty() {
        return String::from("Invalid: empty email");
    }
    
    result
}

