use crate::a::from_a;

pub fn from_b() -> i32 {
    if false { from_a() } else { 0 }
}
