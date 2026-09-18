use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

const TARGET_FILES: &[&str] = &[
    "/data/adb/omk/omkdata/injector.toml",
    "/data/adb/modules/oh_my_keymint/injector.toml",
];

fn wait_for_boot() {
    loop {
        if let Ok(output) = Command::new("getprop").arg("sys.boot_completed").output() {
            if let Ok(val) = std::str::from_utf8(&output.stdout) {
                if val.trim() == "1" {
                    break;
                }
            }
        }
        thread::sleep(Duration::from_secs(2));
    }
}

fn get_installed_packages() -> Result<Vec<String>, String> {
    let output = Command::new("pm")
        .args(["list", "packages"])
        .output()
        .map_err(|e| format!("Failed to execute pm: {}", e))?;

    if !output.status.success() {
        return Err("pm list packages returned non-zero".to_string());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut packages = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(pkg) = trimmed.strip_prefix("package:") {
            let pkg_name = pkg.trim();
            // 过滤系统核心核心包
            if !pkg_name.is_empty()
                && !pkg_name.starts_with("android")
                && !pkg_name.starts_with("com.android")
            {
                packages.push(pkg_name.to_string());
            }
        }
    }

    packages.sort();
    packages.dedup();
    Ok(packages)
}

fn find_scoop_bounds(content: &str) -> Option<(usize, usize)> {
    let mut search_from = 0;
    while let Some(pos) = content[search_from..].find("scoop") {
        let abs_pos = search_from + pos;
        let rest = &content[abs_pos + 5..];
        let mut seen_eq = false;
        let mut bracket_open_offset = None;

        for (i, b) in rest.bytes().enumerate() {
            if b == b'=' && !seen_eq {
                seen_eq = true;
            } else if b == b'[' && seen_eq {
                bracket_open_offset = Some(i + 1);
                break;
            } else if b == b' ' || b == b'\t' {
                continue;
            } else {
                break;
            }
        }

        if let Some(off) = bracket_open_offset {
            let open_pos = abs_pos + 5 + off;
            if let Some(close_rel) = content[open_pos..].find(']') {
                let close_pos = open_pos + close_rel;
                return Some((open_pos, close_pos));
            }
        }
        search_from = abs_pos + 5;
    }
    None
}

fn extract_existing_packages(slice: &str) -> Vec<String> {
    let mut pkgs = Vec::new();
    for line in slice.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let cleaned = trimmed.trim_matches(|c| c == '"' || c == '\'' || c == ',' || c == ' ');
        if !cleaned.is_empty() && cleaned.contains('.') {
            pkgs.push(cleaned.to_string());
        }
    }
    pkgs
}

fn process_file(file_path: &str, installed_pkgs: &[String]) {
    let path = Path::new(file_path);
    if !path.exists() {
        return;
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let (open_pos, close_pos) = match find_scoop_bounds(&content) {
        Some(bounds) => bounds,
        None => return,
    };

    let scoop_slice = &content[open_pos..close_pos];
    let existing_pkgs = extract_existing_packages(scoop_slice);

    // 筛选出不在已有列表中的新应用
    let mut new_pkgs = Vec::new();
    for pkg in installed_pkgs {
        if !existing_pkgs.iter().any(|existing| existing == pkg) {
            new_pkgs.push(pkg.clone());
        }
    }

    if new_pkgs.is_empty() {
        return;
    }

    // 自动适配原文件是否有双引号格式
    let use_quotes = scoop_slice.contains('"');
    let prefix = &content[..open_pos];
    let suffix = &content[open_pos..];

    // 格式化切片：
    // 1. 在 `scoop = [` 后立即写入换行符 `\n`
    // 2. 每行严格两个空格缩进
    let mut insertion = String::new();
    insertion.push('\n');
    for pkg in &new_pkgs {
        insertion.push_str("  ");
        if use_quotes {
            insertion.push('"');
            insertion.push_str(pkg);
            insertion.push_str("\",\n");
        } else {
            insertion.push_str(pkg);
            insertion.push('\n');
        }
    }

    // 去除旧内容开头的换行符，避免多余空行产生
    let trimmed_suffix = if suffix.starts_with("\r\n") {
        &suffix[2..]
    } else if suffix.starts_with('\n') {
        &suffix[1..]
    } else {
        suffix
    };

    let mut updated_content = String::with_capacity(content.len() + insertion.len());
    updated_content.push_str(prefix);
    updated_content.push_str(&insertion);
    updated_content.push_str(trimmed_suffix);

    // 原子写入，防止写入中途断电损坏原文件
    let tmp_path = format!("{}.tmp", file_path);
    if fs::write(&tmp_path, updated_content).is_ok() {
        let _ = fs::rename(&tmp_path, path);
    }
}

fn main() {
    // 设置线程名称并在底层降低调度优先级（nice=19），极致省电
    unsafe {
        let name = std::ffi::CString::new("injector_upd").unwrap();
        libc::prctl(libc::PR_SET_NAME, name.as_ptr());
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }

    wait_for_boot();
    thread::sleep(Duration::from_secs(30));

    loop {
        if let Ok(installed) = get_installed_packages() {
            for file in TARGET_FILES {
                process_file(file, &installed);
            }
        }
        thread::sleep(Duration::from_secs(60));
    }
}
