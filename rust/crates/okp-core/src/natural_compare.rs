use std::cmp::Ordering;

pub fn compare(left: Option<&str>, right: Option<&str>) -> Ordering {
    let Some(left) = left else {
        return if right.is_none() {
            Ordering::Equal
        } else {
            Ordering::Less
        };
    };
    let Some(right) = right else {
        return Ordering::Greater;
    };

    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let mut left_index = 0;
    let mut right_index = 0;

    while left_index < left.len() && right_index < right.len() {
        if left[left_index].is_ascii_digit() && right[right_index].is_ascii_digit() {
            let left_start = left_index;
            while left_index < left.len() && left[left_index].is_ascii_digit() {
                left_index += 1;
            }

            let right_start = right_index;
            while right_index < right.len() && right[right_index].is_ascii_digit() {
                right_index += 1;
            }

            let left_run = left[left_start..left_index].iter().collect::<String>();
            let right_run = right[right_start..right_index].iter().collect::<String>();
            let left_value = left_run.trim_start_matches('0');
            let right_value = right_run.trim_start_matches('0');

            match left_value.len().cmp(&right_value.len()) {
                Ordering::Equal => {}
                ordering => return ordering,
            }

            match left_value.cmp(right_value) {
                Ordering::Equal => {}
                ordering => return ordering,
            }

            match left_run.len().cmp(&right_run.len()) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        } else {
            let left_char = left[left_index].to_uppercase().collect::<String>();
            let right_char = right[right_index].to_uppercase().collect::<String>();

            match left_char.cmp(&right_char) {
                Ordering::Equal => {
                    left_index += 1;
                    right_index += 1;
                }
                ordering => return ordering,
            }
        }
    }

    (left.len() - left_index).cmp(&(right.len() - right_index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_orders_naturally() {
        let cases = [
            ("ep2", "ep10", Ordering::Less),
            ("ep10", "ep2", Ordering::Greater),
            ("a.mkv", "a2.mkv", Ordering::Less),
            ("Show", "show", Ordering::Equal),
            ("ep2", "ep02", Ordering::Less),
            ("file", "file", Ordering::Equal),
        ];

        for (left, right, expected) in cases {
            assert_eq!(compare(Some(left), Some(right)), expected);
        }
    }

    #[test]
    fn compare_orders_none_like_csharp_nulls() {
        assert_eq!(compare(None, None), Ordering::Equal);
        assert_eq!(compare(None, Some("file")), Ordering::Less);
        assert_eq!(compare(Some("file"), None), Ordering::Greater);
    }

    #[test]
    fn sort_puts_episodes_in_human_order() {
        let mut files = ["ep10.mkv", "ep2.mkv", "ep1.mkv", "ep20.mkv", "ep3.mkv"];

        files.sort_by(|left, right| compare(Some(left), Some(right)));

        assert_eq!(
            files,
            ["ep1.mkv", "ep2.mkv", "ep3.mkv", "ep10.mkv", "ep20.mkv"]
        );
    }
}
