//! First file with duplicated code (to trigger duplication detection)

pub fn process_user_data(name: &str, age: i32, email: &str) -> String {
    let validated_name = name.trim().to_uppercase();
    let validated_age = if age < 0 { 0 } else if age > 150 { 150 } else { age };
    let validated_email = email.trim().to_lowercase();
    
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

