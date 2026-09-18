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