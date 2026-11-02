# UnlockProvider

PhoneKey 使用的 Windows 凭证提供程序（Credential Provider）。它运行在 `LogonUI.exe`
中，通过命名管道接收 PhoneKey 应用的解锁授权，并在用户点击锁屏磁贴时把已保存的
Windows 密码交给系统验证。

> 本目录派生自 PsyChip 的 credential provider 项目（MIT，见 [LICENSE](LICENSE)），
> 并针对 PhoneKey 的「点击确认」模型做了修改。

## 与上游的差异

- 锁屏上始终只有 1 个「手机感应解锁」磁贴，不再隐藏磁贴。
- `GetCredentialCount` 返回 `CREDENTIAL_PROVIDER_NO_DEFAULT` 且
  `pbAutoLogonWithDefault = FALSE`，仅靠近不会触发登录。
- 用户名/密码字段设为 `CPFS_HIDDEN`，磁贴上只显示状态文字和提交箭头。
- `SetSelected` 只有在应用已通过管道发送 `UNLOCK` 授权、且距离 1.5 秒启动保护期
  之后才返回 `TRUE`。这避免 Windows 在锁屏出现时恢复上次磁贴导致误解锁。
- 用户点击磁贴或箭头时才调用 `GetSerialization`；没有授权时返回
  `CPGSR_NO_CREDENTIAL_NOT_FINISHED` 并提示「手机不在附近」。
- 应用撤销授权时发送 `CLEAR`，磁贴立即恢复为不可解锁状态。

## 管道协议

| 项目 | 值 |
| --- | --- |
| 管道 | `\\.\pipe\CredentialProviderPipe` |
| 授权 | `UNLOCK:<domain>\<username>:<password>` |
| 撤销 | `CLEAR` |
| 成功响应 | `OK` |
| 失败响应 | `ERR:INVALID_CMD` / `ERR:NO_PROVIDER` / `ERR:PROVIDER_FAILED` |

密码只存在于应用进程与 LogonUI 进程内存中，使用后立即 `SecureZeroMemory`。
管道的 DACL 只允许 SYSTEM、管理员和已认证用户访问，并拒绝远程客户端。

## 构建

需要 Visual Studio 2022（C++ 桌面开发工作负载）：

```bat
build_msvc.bat
test_msvc.bat
```

产物为 `build\UnlockProvider.dll`。`test_msvc.bat` 会加载 DLL，用内存中的
「测试用户/测试密码」走完授权、点击、序列化和撤销流程，不需要真实锁屏。

也支持 MinGW-w64 / TDM-GCC：

```bash
./build_mingw.sh
```

脚本会下载 Windows SDK 10.0.16299.0 的 `credentialprovider.h` 到 `sdkinc/`
（不随仓库提交），并静态链接运行库。`mingw/` 下提供 MinGW 缺失的 IID 定义和
`shobjidl_core.h` 兼容头。

## 安装 / 卸载

PhoneKey 应用内的「安装组件 / 卸载组件」按钮会调用 `installer/` 下的 PowerShell
脚本，并以管理员权限写入 System32 和注册表。也可以手动执行（管理员 PowerShell）：

```powershell
.\installer\install_cp.ps1
.\installer\uninstall_cp.ps1
```

| 项目 | 值 |
| --- | --- |
| CLSID | `{E0A8C5B2-9F3D-4E7A-B1C6-8D2F5A3E9B70}` |
| DLL | `C:\Windows\System32\UnlockProvider.dll` |

安装或更新 DLL 后需要重新锁屏（或重启）才会由 LogonUI 加载新版本。
