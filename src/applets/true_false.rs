pub fn main_true(_: &[String]) -> i32 {
    0
}

pub fn main_false(_: &[String]) -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::{main_false, main_true};

    #[test]
    fn true_returns_zero() {
        assert_eq!(main_true(&[]), 0);
    }

    #[test]
    fn false_returns_one() {
        assert_eq!(main_false(&[]), 1);
    }
}
