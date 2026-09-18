use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

// 需要修改的文件路径
const TARGET_FILES: &[&str] = &[
    "/data/adb/omk/omkdata/injector.toml",
    "/data/adb/modules/oh_my_keymint/injector.toml",
];

/// 等待系统完全开机
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

/// 获取真正的“用户安装应用”列表（完美剔除小米伪装系统应用）
fn get_installed_packages() -> Result<Vec<String>, String> {
    // 1. 获取系统应用列表 (用于做减法)
    let system_output = Command::new("pm")
        .args(["list", "packages", "-s"])
        .output()
        .map_err(|e| format!("Failed to get system packages: {}", e))?;

    let system_text = String::from_utf8_lossy(&system_output.stdout);
    let mut system_pkgs: std::collections::HashSet<String> = std::collections::HashSet::new();
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
            // 3. 核心逻辑：如果在系统列表里，就跳过（做减法）
            // 这样就能把小米伪装成用户应用的 com.miui.* 等系统组件全部干掉
            if !pkg_name.is_empty() && !system_pkgs.contains(pkg_name) {
                installed_pkgs.push(pkg_name.to_string());
            }
        }
    }

    installed_pkgs.sort();
    installed_pkgs.dedup();
    Ok(installed_pkgs)
}

/// 定位 `scoop = [` 和 `]` 的字符索引位置
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
                let close_pos = open_pos + close_rel;
                return Some((open_pos, close_pos));
            }
        }
        search_from = abs_pos + 5;
    }
    None
}

/// 提取现有 scoop 列表中的包名
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

/// 处理单个文件：比对并插入新包名
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

/// 主程序入口
fn main() {
    // 设置线程名称并在底层降低调度优先级（nice=19），极致省电
    unsafe {
        let name = std::ffi::CString::new("injector_upd").unwrap();
        libc::prctl(libc::PR_SET_NAME, name.as_ptr());
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }

    // 等待开机完成
    wait_for_boot();
    // 开机后延迟30秒，等系统稳定
    thread::sleep(Duration::from_secs(30));

    // 进入无限循环
    loop {
        if let Ok(installed) = get_installed_packages() {
            for file in TARGET_FILES {
                process_file(file, &installed);
            }
        }
        // 每 60 秒扫描一次
        thread::sleep(Duration::from_secs(60));
    }
}