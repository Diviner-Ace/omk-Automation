# OMK Injector Auto-Updater

自动监控已安装应用并追加至 Oh My Keymint (OMK) `injector.toml` 的守护进程。

## 部署指南

### 1. 放置二进制与脚本
通过 ADB Shell 或 Root 文件管理器创建目录并放入文件：

```bash
mkdir -p /data/adb/injector_updater
mkdir -p /data/adb/service.d
