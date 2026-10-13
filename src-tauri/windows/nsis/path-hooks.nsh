; UFO —— 安装器负责用户 PATH 的追加与摘除；App 运行期完全不碰环境变量与注册表。
; 与 ../wix/path-fragment.wxs 同语义（MSI 那条通道）。
;
; 本文件由 tauri.conf.json 的 bundle.windows.nsis.installerHooks 挂进模板；
; 模板只在四处 `!ifmacrodef` 里插入同名宏，所以这里只定义宏、不写裸语句。
;
; ── 三条禁区（改这个文件前先读）────────────────────────────────────────────
; 1. 绝对不许出现 `nsExec::Exec 'setx PATH ...'`。setx 把值截到 1024 字符再写回，
;    实测本机用户 PATH raw 已 387 字符 / 12 段，一次就够把别人的条目拦腰砍断，且不可逆。
; 2. 写 PATH 必须 `WriteRegExpandStr`、读必须读 raw。HKCU\Environment\Path 本机实测是
;    ExpandString（raw 387 / expanded 405，含 %NVM_HOME% %NVM_SYMLINK%）；把展开值写回去
;    等于将间接引用固化成死路径 —— 换 node 版本后 PATH 指向一个不存在的目录。
; 3. 卸载侧只能用 NSIS_HOOK_POSTUNINSTALL，不能用 PREUNINSTALL：后者插在
;    installer.nsi:727，紧接着 :729 就是 `CheckIfAppIsRunning`，应用正在跑时会 Abort ——
;    那时 PATH 已经被摘掉，安装却还完好。
;
; ── 寄存器预算（模板在插入点前后自己也在用，撞了会静默出错）──────────────────
; 安装侧用 $R1-$R9：模板在 POSTINSTALL 之后（:697-701）用 ${GetOptions} 写 $R0，别占它。
; 卸载侧用 $R0-$R9：:815 之后模板只读 Var（PassiveMode/UpdateMode），寄存器全空。

; 广播用的消息号与目标句柄自己带 !ifndef 兜住，不 !include WinMessages.nsh —— 模板已经
; include 了 MUI2/WordFunc/StrFunc 一大串，重复 include 有撞 !define 的风险。
!ifndef UFO_HWND_BROADCAST
  !define UFO_HWND_BROADCAST 0xFFFF
!endif
!ifndef UFO_WM_SETTINGCHANGE
  !define UFO_WM_SETTINGCHANGE 0x1A
!endif
; 环境变量落点做成可覆盖的两个 define：默认就是 Windows 真正的用户 PATH。
; 单测（探针脚本）在 !include 本文件之前把它们改成 `HKCU\Environment\ufotest` 这种
; scratch 子键，于是同一段算法能端到端跑、又不碰到任何人真实的 PATH。
!ifndef UFO_ENV_ROOT
  !define UFO_ENV_ROOT HKCU
!endif
!ifndef UFO_ENV_SUBKEY
  !define UFO_ENV_SUBKEY "Environment"
!endif

; 让 Explorer 这类会主动重读环境变量的进程刷新。注意它刷新不了已经在跑的 agent 宿主进程
; （子进程环境块在 CreateProcess 时复制一份），所以文档仍然要求装完重启宿主。
; 形参口径：HWND 用 p 且值就是 HWND_BROADCAST 的 0xFFFF（不是 -1），lpdwResult 用 *i.r0
; 才是「按引用收」，写成 `i .r0` 会变成按值传一个整数进去当指针；宽串与模板 :426 /
; utils.nsh:82 的 System::Call `w "..."` 同源。
!macro UFO_BROADCAST_ENV
  System::Call 'user32::SendMessageTimeoutW(p ${UFO_HWND_BROADCAST}, i ${UFO_WM_SETTINGCHANGE}, p 0, w "Environment", i 2, i 3000, *i.r0) i.r1'
!macroend

; ============ 安装：把 $INSTDIR 追加到用户 PATH（已在里面就一个字节都不写）============
!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $R9 ${UFO_ENV_ROOT} ${UFO_ENV_SUBKEY} "Path"   ; raw，非展开值
  StrCpy $R8 "$INSTDIR"                      ; 要找 / 要追加的那一段
  StrCpy $R7 ""                              ; found 标志
  StrCpy $R6 "$R9"                           ; 剩余待扫部分
ufo_i_tok:
  StrCpy $R5 ""                              ; 当前段内容
  StrCpy $R4 0                               ; 段内游标
ufo_i_char:
  StrCpy $R3 "$R6" 1 $R4                     ; 取 1 个字符；游标越界即空串 = 段结束
  ${If} $R3 == ""
    Goto ufo_i_endtok
  ${EndIf}
  ${If} $R3 == ";"
    Goto ufo_i_endtok
  ${EndIf}
  StrCpy $R5 "$R5$R3"
  IntOp $R4 $R4 + 1
  Goto ufo_i_char
ufo_i_endtok:
  ${If} $R5 == $R8                           ; 精确整段相等才算命中，前缀相同不算
    StrCpy $R7 "1"
  ${EndIf}
  IntOp $R4 $R4 + 1                          ; 跳过分隔符（段尾时是越界，无副作用）
  StrCpy $R2 "$R6" "" $R4                    ; 余下部分
  StrCpy $R6 $R2
  ${If} $R6 != ""
    Goto ufo_i_tok
  ${EndIf}

  ${If} $R7 == "1"
    ; 幂等：同版本覆盖装 / 修复装都会走到这里。不写注册表，也就不可能把 PATH 写出漂移。
    DetailPrint "UFO: install dir already on user PATH, skipped"
  ${Else}
    ${If} $R9 == ""
      StrCpy $R1 "$R8"
    ${Else}
      StrCpy $R1 "$R9;$R8"                   ; 只追加，绝不整串替换
    ${EndIf}
    WriteRegExpandStr ${UFO_ENV_ROOT} ${UFO_ENV_SUBKEY} "Path" $R1
    !insertmacro UFO_BROADCAST_ENV
    DetailPrint "UFO: added $INSTDIR to user PATH"
  ${EndIf}
!macroend

; ============ 卸载：从用户 PATH 精确摘掉 $INSTDIR 那一段 ============
; 时机选 POSTUNINSTALL：此时文件已删、`CheckIfAppIsRunning` 已通过，不会再出现
; 「PATH 摘了但应用还在」。更新（旧卸载器被静默调起）时 $UpdateMode = 1，
; 与模板 :752 / :793 / :799 一样跳过 —— 紧随其后的新安装会重新追加，摘它纯属多余。
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ReadRegStr $R9 ${UFO_ENV_ROOT} ${UFO_ENV_SUBKEY} "Path"
    StrCpy $R8 "$INSTDIR"
    StrCpy $R1 ""                            ; found 标志
    StrCpy $R0 ""                            ; 摘除后的结果串
    StrCpy $R5 0                             ; 当前段起点
    StrCpy $R4 0                             ; 全局游标
    StrCpy $R2 ""                            ; 当前段内容
  ufo_u_char:
    StrCpy $R3 "$R9" 1 $R4                   ; 当前字符
    ${If} $R3 == ";"
      ${If} $R1 == ""
      ${AndIf} $R2 == $R8
        StrCpy $R1 "1"
        Goto ufo_u_hit
      ${EndIf}
      StrCpy $R2 ""
      IntOp $R5 $R4 + 1                      ; 下一段起点
      IntOp $R4 $R5 + 0
      Goto ufo_u_char
    ${EndIf}
    ${If} $R3 == ""                          ; 扫到串尾
      ${If} $R1 == ""
      ${AndIf} $R2 == $R8
        StrCpy $R1 "1"
        Goto ufo_u_hit
      ${EndIf}
      Goto ufo_u_absent
    ${EndIf}
    StrCpy $R2 "$R2$R3"
    IntOp $R4 $R4 + 1
    Goto ufo_u_char

    ; 命中：删除区间 = [$R5, $R4)。段后紧跟分号就一起吃掉；是最后一段就改吃前面那个，
    ; 这样其余部分逐字符原样保留（不用「拆段再 join」，那种写法会吃掉 PATH 里本就存在的
    ; 空段 / 尾分号，卸载后就回不到装之前的字节）。
  ufo_u_hit:
    StrCpy $R6 $R4                           ; 删除区间末尾
    StrCpy $R3 "$R9" 1 $R4
    ${If} $R3 != ";"
      IntOp $R5 $R5 - 1                      ; 往前吃掉分隔符
      ${If} $R5 < 0
        StrCpy $R5 0
      ${EndIf}
    ${Else}
      IntOp $R6 $R6 + 1
    ${EndIf}
    StrCpy $R0 "$R9" $R5                     ; 前段（从头取 $R5 个字符）
    StrCpy $R7 "$R9" "" $R6                  ; 后段
    StrCpy $R0 "$R0$R7"

    ${If} $R0 == ""
      DeleteRegValue ${UFO_ENV_ROOT} ${UFO_ENV_SUBKEY} "Path"
      !insertmacro UFO_BROADCAST_ENV
      DetailPrint "UFO: user PATH became empty, removed the value"
    ${Else}
      WriteRegExpandStr ${UFO_ENV_ROOT} ${UFO_ENV_SUBKEY} "Path" $R0
      !insertmacro UFO_BROADCAST_ENV
      DetailPrint "UFO: removed $INSTDIR from user PATH"
    ${EndIf}
    Goto ufo_u_done

    ; 没找到：一个字节都不写。用户手工改过那一条（大小写、尾斜杠）时会落到这里 ——
    ; 宁可留一条指向已删目录的残留（Windows 直接忽略），也不冒险误删别人的条目。
  ufo_u_absent:
    DetailPrint "UFO: no PATH entry for $INSTDIR, left user PATH untouched"
  ufo_u_done:
  ${EndIf}
!macroend
