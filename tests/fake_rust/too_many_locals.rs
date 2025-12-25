//! A function with too many local variables (threshold: 10)

pub fn variable_hoarder() -> i32 {
    let var1 = 1;
    let var2 = 2;
    let var3 = 3;
    let var4 = 4;
    let var5 = 5;
    let var6 = 6;
    let var7 = 7;
    let var8 = 8;
    let var9 = 9;
    let var10 = 10;
    let var11 = 11;
    let var12 = 12;
    let var13 = 13;
    let var14 = 14;
    let var15 = 15;
    
    var1 + var2 + var3 + var4 + var5 + var6 + var7 + var8 + var9 + var10 
        + var11 + var12 + var13 + var14 + var15
}

pub fn also_hoarding_variables(input: i32) -> i32 {
    let a = input + 1;
    let b = input + 2;
    let c = input + 3;
    let d = input + 4;
    let e = input + 5;
    let f = input + 6;
    let g = input + 7;
    let h = input + 8;
    let i = input + 9;
    let j = input + 10;
    let k = input + 11;
    let l = input + 12;
    
    a * b + c * d + e * f + g * h + i * j + k * l
}

