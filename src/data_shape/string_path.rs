use std::fmt;

const VERBATIM_PREFIX: &str = r#"\\?\"#;

fn is_windows_path_start(s: &str) -> bool {
    let mut chars = s.chars();
    if let (Some(c0), Some(c1)) = (chars.next(), chars.next()) {
        c0.is_ascii_alphabetic() && c1 == ':'
    } else {
        false
    }
}

pub fn join_path<P: AsRef<str> + fmt::Display>(path_parent: P, path_child: P) -> String {
    let pp = path_parent.as_ref();
    if pp.starts_with(VERBATIM_PREFIX) { // it's a windows path.
        let (_, pp1) = pp.split_at(4);
        format!("{}\\{}", pp1, path_child)
    } else if is_windows_path_start(pp) {
        format!("{}\\{}", pp, path_child)
    } else {
        format!("{}/{}", pp, path_child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t_idx() {
        let s = "a:\\b";
        let c0 = s.chars().nth(0).expect("at least have one char.");
        let c1 = s.chars().nth(1).expect("at least have one char.");
        assert!(c0.is_ascii_alphabetic());
        assert_eq!(c1, ':');
    }

    #[test]
    fn t_join() {
        let pp = r#"\\?\D:\Documents\GitHub\ssh-client-demo\fixtures\adir"#;
        let ch = "a.txt";
        let j = join_path(pp, ch);
        assert_eq!(j, r#"D:\Documents\GitHub\ssh-client-demo\fixtures\adir\a.txt"#);


        let pp = r#"D:\Documents\GitHub\ssh-client-demo\fixtures\adir"#;
        let ch = "a.txt";
        let j = join_path(pp, ch);
        assert_eq!(j, r#"D:\Documents\GitHub\ssh-client-demo\fixtures\adir\a.txt"#);


        let pp = r#":\Documents\GitHub\ssh-client-demo\fixtures\adir"#;
        let ch = "a.txt";
        let j = join_path(pp, ch);
        assert_eq!(j, r#":\Documents\GitHub\ssh-client-demo\fixtures\adir/a.txt"#);
    }

}