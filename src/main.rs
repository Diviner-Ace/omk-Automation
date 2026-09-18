use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::collections::HashSet;

const TARGET_FILES: &[&str] = &[
    "/data/adb/omk/omkdata/injector.toml",
    "/data/adb/modules/oh_my_keymint/injector.toml",
];

// 新增：初始快照文件（记录所有曾经出现过的应用）
const BASELINE_FILE: &str = "/data/adb/injector_updater/baseline.txt";

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
    // 1. 获取系统应用列表 (用于做减法)
    let system_output = Command::new("pm")
        .args(["list", "packages", "-s"])
        .output()
        .map_err(|e| format!("Failed to get system packages: {}", e))?;
    let system_text = String::from_utf8_lossy(&system_output.stdout);
    let mut system_pkgs: HashSet<String> = HashSet::new();
    for line in system_text.lines() {
        if let Some(pkg) = line.trim().strip_prefix("package:") {
            system_pkgs.insert(pkg.trim().to_string());
        }
    }

    // 2. 获取第三方用户应用列表
    let user_output = Command::new("pm")
        .args(["list", "packages", "-3"])
        .output()
        .map_err(|e| format!("Failed to get user packages: {}", e))?;

    if !user_output.status.success() {
        return Err("pm list packages -3 returned non-zero".to_string());
    }

    let user_text = String::from_utf8_lossy(&user_output.stdout);
    let mut installed_pkgs = Vec::new();

    for line in user_text.lines() {
        let trimmed = line.trim();
        if let Some(pkg) = trimmed.strip_prefix("package:") {
            let pkg_name = pkg.trim();
            // 剔除伪装成用户应用的系统组件
            if !pkg_name.is_empty() && !system_pkgs.contains(pkg_name) {
                installed_pkgs.push(pkg_name.to_string());
            }
        }
    }
    installed_pkgs.sort();
    installed_pkgs.dedup();
    Ok(installed_pkgs)
}

/// 核心：读取 baseline 快照，对比找出真正的“新安装应用”
fn get_newly_installed_packages(installed_pkgs: &[String]) -> Vec<String> {
    let baseline_path = Path::new(BASELINE_FILE);

    // 如果快照文件不存在，说明是第一次运行
    if !baseline_path.exists() {
        if let Ok(mut file) = fs::File::create(baseline_path) {
            for pkg in installed_pkgs {
                let _ = writeln!(file, "{}", pkg);
            }
        }
        // 首次运行，把当前所有应用全部视为新应用，让它们先加到 TOML 里
        return installed_pkgs.to_vec();
    }

    let baseline_content = match fs::read_to_string(baseline_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let existing_baseline: HashSet<String> = baseline_content
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut newly_installed = Vec::new();
    for pkg in installed_pkgs {
        if !existing_baseline.contains(pkg) {
            newly_installed.push(pkg.clone());
        }
    }

    // 如果有新应用，立刻把它们追加到快照文件里，防止程序崩溃导致重复记录
    if !newly_installed.is_empty() {
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(baseline_path) {
            for pkg in &newly_installed {
                let _ = writeln!(file, "{}", pkg);
            }
        }
    }

    newly_installed
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
            } else if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                continue;
            } else {
                break;
            }
        }

        if let Some(off) = bracket_open_offset {
            let open_pos = abs_pos + 5 + off;
            if let Some(close_rel) = content[open_pos..].find(']') {
                return Some((open_pos, open_pos + close_rel));
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
        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
        let cleaned = trimmed.trim_matches(|c| c == '"' || c == '\'' || c == ',' || c == ' ');
        if !cleaned.is_empty() && cleaned.contains('.') {
            pkgs.push(cleaned.to_string());
        }
    }
    pkgs
}

/// 注意：这里的 `new_pkgs` 参数只接收“真正新安装”的包名
fn process_file(file_path: &str, new_pkgs: &[String]) {
    let path = Path::new(file_path);
    if !path.exists() || new_pkgs.is_empty() { return; }

    let content = match fs::read_to_string(path) { Ok(c) => c, Err(_) => return };
    let (open_pos, close_pos) = match find_scoop_bounds(&content) { Some(b) => b, None => return };

    let scoop_slice = &content[open_pos..close_pos];
    let existing_pkgs = extract_existing_packages(scoop_slice);

    // 过滤掉 TOML 里已经存在的（防止快照和 TOML 不一致导致重复写入）
    let mut final_new = Vec::new();
    for pkg in new_pkgs {
        if !existing_pkgs.iter().any(|e| e == pkg) {
            final_new.push(pkg.clone());
        }
    }

    if final_new.is_empty() { return; }

    let use_quotes = scoop_slice.contains('"');
    let prefix = &content[..open_pos];
    let suffix = &content[open_pos..];

    let mut insertion = String::new();
    insertion.push('\n');
    for pkg in &final_new {
        insertion.push_str("  ");
        if use_quotes {
            insertion.push('"'); insertion.push_str(pkg); insertion.push_str("\",\n");
        } else {
            insertion.push_str(pkg); insertion.push('\n');
        }
    }

    let trimmed_suffix = if suffix.starts_with("\r\n") { &suffix[2..] }
                         else if suffix.starts_with('\n') { &suffix[1..] }
                         else { suffix };

    let mut updated_content = String::with_capacity(content.len() + insertion.len());
    updated_content.push_str(prefix);
    updated_content.push_str(&insertion);
    updated_content.push_str(trimmed_suffix);

    let tmp_path = format!("{}.tmp", file_path);
    if fs::write(&tmp_path, updated_content).is_ok() {
        let _ = fs::rename(&tmp_path, path);
    }
}

fn main() {
    unsafe {
        let name = std::ffi::CString::new("injector_upd").unwrap();
        libc::prctl(libc::PR_SET_NAME, name.as_ptr());
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }

    wait_for_boot();
    thread::sleep(Duration::from_secs(30));

    loop {
        if let Ok(installed) = get_installed_packages() {
            // 1. 从 baseline 快照里找出真正的新应用
            let newly_installed = get_newly_installed_packages(&installed);
            
            // 2. 如果有新应用，同时写入 TOML 和更新快照（快照已在函数内更新）
            if !newly_installed.is_empty() {
                for file in TARGET_FILES {
                    process_file(file, &newly_installed);
                }
            }
        }
        thread::sleep(Duration::from_secs(60));
    }
}