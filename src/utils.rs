use std::path::Path;

pub fn readable_path(path: &str, max_len: usize) -> String {
    let p = Path::new(path);
    let parts: Vec<_> = p.components().collect();
    let s = path.to_string();
    if s.len() <= max_len {
        return s;
    }
    if parts.len() <= 3 {
        return s;
    }
    let first = parts[0].as_os_str().to_string_lossy().to_string();
    let last = parts[parts.len() - 1]
        .as_os_str()
        .to_string_lossy()
        .to_string();
    for n in (1..parts.len() - 1).rev() {
        let mut shortened = first.clone();
        shortened.push_str("/.../");
        for part in parts[parts.len() - n..].iter() {
            shortened.push_str(&part.as_os_str().to_string_lossy());
            shortened.push('/');
        }
        shortened.pop();
        if shortened.len() <= max_len {
            return shortened;
        }
    }
    format!("{}/.../{}", first, last)
}
