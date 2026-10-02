use crate::api::handler;

pub fn rule() -> i32 {
    handler()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works() {
        assert_eq!(rule(), 1);
    }
}
