//! A function with too many arguments (threshold: 8)

pub fn too_many_parameters(
    arg1: i32,
    arg2: i32,
    arg3: i32,
    arg4: i32,
    arg5: i32,
    arg6: i32,
    arg7: i32,
    arg8: i32,
    arg9: i32,
    arg10: i32,
) -> i32 {
    arg1 + arg2 + arg3 + arg4 + arg5 + arg6 + arg7 + arg8 + arg9 + arg10
}

pub fn also_too_many(
    a: String,
    b: String,
    c: String,
    d: String,
    e: String,
    f: String,
    g: String,
    h: String,
    i: String,
) -> String {
    format!("{}{}{}{}{}{}{}{}{}", a, b, c, d, e, f, g, h, i)
}

