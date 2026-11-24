; PhoneKey NSIS installer hooks
!macro NSIS_HOOK_POSTINSTALL
  ${If} $PassiveMode = 0
    ${IfNot} ${Silent}
      ${IfNot} ${FileExists} "$WINDIR\System32\vcruntime140.dll"
        MessageBox MB_ICONEXCLAMATION|MB_OK "未检测到 Microsoft Visual C++ 运行库（VCRUNTIME140.dll）。安装 PhoneKey 的锁屏解锁组件前，请先安装 Microsoft Visual C++ 2015–2022 x64 运行库，否则锁屏界面不会显示手机解锁选项。"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
