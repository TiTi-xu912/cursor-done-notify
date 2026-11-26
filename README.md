# 点点

A Windows desktop buddy for [Cursor](https://cursor.com). It follows an Agent turn through Cursor hooks and stays on screen while the agent reads, edits, and runs commands. When the turn finishes, it plays a sound and shows a short snippet of the reply. A bar at the bottom shows your Cursor usage for the billing cycle. The UI follows the Windows display language (Chinese or English) and can be switched from the right-click menu.

<img src="docs/screenshots/english.png" width="320" alt="English UI">

点点是 Cursor Agent 的 Windows 桌面伙伴：一个戴耳机、穿卫衣的 Q 版程序员。Agent 开始干活后，它出现在桌面右下角，跟着读文件、改代码、跑命令换动作；这一轮结束、中断或出错时响一声，并带上回答摘要。多个对话同时进行时，它收成一张总览。气泡底部常驻一条 Cursor 额度栏，点开能看本期用量详情。

## 效果

窗口本身是透明的，贴在桌面右下角。

| 正在思考 | 正在读取 | 正在执行命令 |
| --- | --- | --- |
| <img src="docs/screenshots/think.png" width="240" alt="正在思考"> | <img src="docs/screenshots/read.png" width="240" alt="正在读取"> | <img src="docs/screenshots/terminal.png" width="240" alt="正在执行命令"> |

| 正在修改 | 正在搜索 | 点开最近步骤 |
| --- | --- | --- |
| <img src="docs/screenshots/write.png" width="240" alt="正在修改"> | <img src="docs/screenshots/search.png" width="240" alt="正在搜索"> | <img src="docs/screenshots/steps.png" width="240" alt="展开步骤"> |

| 回答结束 | 这一轮出错 | 多个会话 |
| --- | --- | --- |
| <img src="docs/screenshots/done.png" width="240" alt="回答结束"> | <img src="docs/screenshots/error.png" width="240" alt="出错"> | <img src="docs/screenshots/board.png" width="240" alt="多个会话"> |

| 额度详情 | 额度提醒 | 深色主题 |
| --- | --- | --- |
| <img src="docs/screenshots/quota.png" width="240" alt="额度详情"> | <img src="docs/screenshots/alert.png" width="240" alt="额度提醒"> | <img src="docs/screenshots/dark.png" width="240" alt="深色主题"> |

## 动作

每个状态各有一段循环动画。

| 空闲 | 思考 | 读取 | 修改 |
| --- | --- | --- | --- |
| <img src="sprites/poses/idle.gif" width="120" alt="空闲"> | <img src="sprites/poses/think.gif" width="120" alt="思考"> | <img src="sprites/poses/read.gif" width="120" alt="读取"> | <img src="sprites/poses/write.gif" width="120" alt="修改"> |

| 命令 | 搜索 | 完成 | 出错 |
| --- | --- | --- | --- |
| <img src="sprites/poses/terminal.gif" width="120" alt="命令"> | <img src="sprites/poses/search.gif" width="120" alt="搜索"> | <img src="sprites/poses/done.gif" width="120" alt="完成"> | <img src="sprites/poses/error.gif" width="120" alt="出错"> |

| 派子任务 | 查资料 | 已中断 | 打瞌睡 |
| --- | --- | --- | --- |
| <img src="sprites/poses/delegate.gif" width="120" alt="派子任务"> | <img src="sprites/poses/web.gif" width="120" alt="查资料"> | <img src="sprites/poses/stop.gif" width="120" alt="已中断"> | <img src="sprites/poses/sleep.gif" width="120" alt="打瞌睡"> |

查资料也用于网页搜索和外部（MCP）工具。一轮进行中超过 90 秒没有新事件、也没有工具在跑时，点点会打瞌睡；会话失去连接时也是这个动作。

这是一个非官方工具，和 Cursor 没有隶属关系。

## 需要什么

- Windows 10 或更新版本
- 系统自带的 .NET Framework 4.x（安装脚本用到系统自带的 Windows PowerShell 5.1）
- 桌面版 Cursor，并且能使用 Hooks

不需要单独安装 Node.js、Python 或 .NET SDK。`install.cmd` 会用 Windows 自带的 `csc.exe` 编译两个小程序；读取额度用的是 Windows 自带的 `winsqlite3.dll`。

## 安装

1. 从 [Releases](https://github.com/TiTi-xu912/cursor-done-notify/releases) 下载最新的 zip 解压，或者克隆这个仓库，放到一个你打算一直留着的目录。Hook 会按这个路径启动点点，之后不要把文件夹挪走。
2. 双击 `install.cmd`。
3. 完全退出 Cursor，再重新打开。
4. 开始一轮 Agent 对话。点点会出现在鼠标所在屏幕的右下角。

安装程序写入的是用户级配置 `%USERPROFILE%\.cursor\hooks.json`。原来已经存在的其他 hook 会保留，只会换掉点点自己的条目。

双击安装默认使用**安全模式**：不注册需要返回权限决定的 Hook，因此不会替 Cursor 允许工具执行；读、写、搜索和命令动作会在相应工具完成后显示。

如果希望动作从工具开始时就实时变化，可以显式启用实时模式：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1 -EnableRealtime
```

实时模式会注册 `preToolUse` 和 `subagentStart`，并按照 Cursor Hook 协议返回 `permission: "allow"`。这会参与 Cursor 的工具权限决策，只应在你理解并接受这一行为时启用。重新运行普通的 `install.cmd` 会切回安全模式。

更新配置前会在同目录保留一份 `hooks.json.bak`；写入使用临时文件替换，避免中途退出损坏配置。

### Rust 版（预览）

`proto/buddy-rs` 里是用 Rust 重写的同一套程序，功能和行为与默认版本一致，常驻内存约 16 MB（默认版本约 60 MB），中继每次调用也更快。它也是以后支持 macOS 的基础。

- 用 zip 安装：下载 Releases 里名字带 `-rust` 的 zip，解压后双击 `install.cmd`，不需要装 Rust。
- 用 git 安装：装好 [Rust](https://rustup.rs) 后运行 `powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1 -Engine rust`（实时模式再加 `-EnableRealtime`）。

两个版本读写同一份设置、缓存和收件箱，可以随时切换；在 git 目录里直接双击 `install.cmd` 会换回默认版本。

### macOS（预览）

Rust 版也能在 macOS 11 或更新版本上运行，Apple 芯片和 Intel 都支持。目前只在 CI 的 Mac 机器上自动测试过（启动、跟着一轮对话走、安装和卸载），还没有在真机上手动用过。

1. 下载 Releases 里名字带 `-macos` 的 zip，解压到一个打算一直留着的目录。
2. 在「终端」里运行：

   ```bash
   bash ~/路径/cursor-done-notify-v1.2.0-macos/install.sh
   ```

   需要实时动作时加上 `--realtime`。用 git 的话，装好 [Rust](https://rustup.rs) 后在仓库里运行 `bash install.sh`，它会先编译。
3. 完全退出 Cursor 再打开。

程序没有用 Apple 开发者证书签名。`install.sh` 会去掉下载带来的隔离标记，这样 Gatekeeper 不会拦它；如果你更想手动处理，也可以在「系统设置 → 隐私与安全性」里选择仍要打开。卸载运行同一目录下的 `uninstall.sh`。

和 Windows 版的差别：

- 托盘图标在屏幕顶部的菜单栏里，点它打开菜单；在点点身上点右键也有同样的菜单。
- 点标题跳回对话时，会用 `open -a Cursor <项目目录>` 把对应的 Cursor 窗口调到前面。终端里跑的 Cursor CLI 暂时跳不回去。
- 「正看着对话时不打扰」：macOS 不给普通程序读取别的窗口标题，所以只要 Cursor 在最前面就算你在看，不区分是哪个项目的窗口。
- 新版本提示只出现在菜单最上面，不弹系统通知。

## 使用

- 拖动窗口可以搬家，松手后位置会记住。
- 点角色会戳一下。进行中点窗口其他位置，可以展开或收起最近几步。
- 点标题跳回这个对话：在终端里用 Cursor CLI 时切到那个终端窗口，在 Cursor 里时切到对应项目的窗口。
- 这一轮结束、中断或出错后，点窗口就能关掉；也可以等它自己消失。
- 右上角的关闭同样有效。
- 好几个对话同时在跑时，窗口变成总览。点一行就盯住这一路，并尝试把对应工作区的 Cursor 窗口调到前面；再点一次回到总览。已经结束的那一行，点一下就从总览里去掉。
- 右键点窗口，或者点任务栏托盘里的点点图标，会弹出菜单：静音、正看着对话时不打扰、显示额度栏、主题（跟随系统、浅色、深色）、结束后停留多久、多久没动静算失联、语言（跟随系统、中文、English）、打开 Cursor 用量页、先关掉点点。这些选择保存在 `%APPDATA%\cursor-done-notify\settings.txt`，立即生效，不用重启 Cursor。

结束时的声音沿用 Windows 系统音：正常完成、中断、出错各一声。静音后不再响。如果一轮结束时你正看着这个对话（前台就是跑它的终端或 Cursor 窗口），默认不响，气泡 4 秒后就收起。

主题默认跟随 Windows 的“应用模式”，深色模式下气泡变深。

默认停留 12 秒，可以在右键菜单的“结束后停留”里改。也可以设置用户环境变量 `CURSOR_NOTIFY_SECONDS`（允许 3 到 60），菜单里选过的值优先：

```powershell
[Environment]::SetEnvironmentVariable('CURSOR_NOTIFY_SECONDS', '20', 'User')
```

如果 Cursor 意外退出，长时间收不到事件的会话默认会在 180 分钟后标成“会话失去连接”。可以在右键菜单的“多久没动静算失联”里改，或者用 `CURSOR_NOTIFY_STALE_MINUTES` 调整为 10 到 1440 分钟：

```powershell
[Environment]::SetEnvironmentVariable('CURSOR_NOTIFY_STALE_MINUTES', '60', 'User')
```

## 额度

气泡底部的额度栏显示本期综合用量、指定模型（API）用量和离重置还有几天；进度条超过 70% 变橙，超过 90% 变红。点一下额度栏展开详情：套餐名、账单周期、Auto 与 API 两条用量、套餐内金额和赠送金额、更新时间，再点一下收起。

数据来自 Cursor 自己的用量接口（`api2.cursor.sh` 上的 `DashboardService`），和 Cursor 设置页看到的是同一份。点点从本机 `%APPDATA%\Cursor\User\globalStorage\state.vscdb` 读取 Cursor 已保存的登录令牌，只发给 Cursor 的服务器，不保存也不发给别处。窗口出现时拉一次，之后每 5 分钟、每轮结束时、以及展开详情时刷新（两次之间至少隔 20 秒）。结果缓存在 `%TEMP%\cursor-done-notify\quota-cache.txt`，只有数字，没有令牌；下次出现时先显示缓存。

一轮结束时，元信息行会多一段“本轮约 $0.42”：这一轮开始后第一次拉到的已用金额，和结束后最新一次的差。Cursor 记账有延迟，最后几次请求可能还没算进去，所以只是估算；如果同时还有别的对话在跑，会写成“期间约”，因为额度是整个账号共用的。

综合用量或 API 用量第一次超过 80%、第一次用完时，提示行会变成橙色提醒并响一声，每个账单周期每档只提醒一次。

不想看额度栏，在右键菜单里取消“显示额度栏”即可。想彻底不查询额度（也就不会有每轮花费和提醒），设置：

```powershell
[Environment]::SetEnvironmentVariable('CURSOR_NOTIFY_QUOTA', '0', 'User')
```

## 更新

点点每天最多向 GitHub 查一次有没有新的 Release（`api.github.com`，只带版本号，不带任何对话内容）。有新版本时，托盘会弹一次提示，右键菜单最上面会出现“发现新版本”，点它打开下载页。不想检查，在右键菜单里取消“自动检查更新”，或者设置 `CURSOR_NOTIFY_UPDATES=0`。

更新方法：

- 用 zip 安装的：先在右键菜单里选“先关掉点点”，再把新 zip 解压覆盖到原来的目录，双击 `install.cmd`。
- 用 git 的：`git pull` 之后双击 `install.cmd`，它会重新编译。

## 卸载

双击 `uninstall.cmd`。它会关掉点点，只从 `hooks.json` 里删掉当前安装目录对应的条目，并清理临时事件、摘要和错误日志；窗口位置和右键菜单里的设置会保留。然后完全退出 Cursor 再打开。

## 它怎么工作

```text
Cursor Agent
    │  stdin 上的 hook JSON
    ▼
hook-relay.exe          立刻回复 {} / allow / continue
    │  截短后写入 %TEMP%\cursor-done-notify\inbox
    │  每轮第一个事件顺带记下跑这个对话的窗口（终端或 Cursor）
    ▼
buddy-host.exe          监听收件箱，按对话分开记状态，画点点，查额度
```

`buddy-host.exe` 由 `Host.cs`（状态机）、`HostApp.cs`（窗口、托盘、自测、预览）、`BuddyForm.cs`（绘制）和 `Quota.cs`（额度）编译而成。它只在有对话时运行，所有对话都结束、窗口消失后就退出。

安全模式注册：`beforeSubmitPrompt`、`postToolUse`、`postToolUseFailure`、`afterAgentThought`、`afterAgentResponse`、`subagentStop`、`stop`。实时模式另外注册 `preToolUse` 和 `subagentStart`。

事件只留在本机临时目录里。过长的提示、命令和回答会截短后再交给窗口，超过一天的残留事件会在下次启动时清理。点点不会把这些内容发出去；网络请求只有上面说的额度查询和每天一次的更新检查。

## 开发

`tests/hooks.ps1` 会先通过 `tests/host.ps1` 把宿主编译到临时目录，跑状态机自测（`buddy-host.exe --selftest`）和额度解析检查（不联网），再检查中继写出的事件，最后用一个临时用户目录检查安装和卸载：别人的 hook 还在，点点的条目会被去掉。它不会改你自己的 `%USERPROFILE%\.cursor\hooks.json`。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\hooks.ps1
```

改完任何 `.cs` 文件后，再运行一次 `install.cmd`。

Rust 版的测试在 `proto/buddy-rs` 里运行 `cargo test`（状态机、额度、更新检查和中继），中继和安装的端到端检查是 `tests\relay-rs.ps1`。`buddy-host.exe --snapshot <目录>` 会把几种典型画面画成 PNG，不打开窗口。

发布新版本：先把 `Host.cs` 里的 `AppInfo.Version` 改成新版本号并提交，再推送同名 tag（比如 `v1.2.0`）。`.github/workflows/release.yml` 会跑测试，用 `tools/package.ps1` 编译并打包，然后创建 Release。tag 和代码里的版本号对不上时打包会失败。本地也可以直接运行 `tools\package.ps1`，zip 写在 `dist` 目录。编译产物 `hook-relay.exe` 和 `buddy-host.exe` 不进版本库。

要重新生成 README 里的截图，运行 `buddy-host.exe --preview <目录>`，不给目录时写到 `%TEMP%\cursor-done-notify\preview`。

角色的原始图在 `docs/art`：`character.jpg` 是定稿，`pose-<动作>.jpg` 是每个动作并排的 4 帧，背景是纯绿 `#00FF00`。换了其中某张以后，运行下面的命令重新抠图、切帧、按鞋底对齐，并重新生成 `sprites/poses` 里的 PNG 和 GIF。这一步只有开发时需要，要装 Python 3、Pillow 和 numpy：

```powershell
python tools\build-sprites.py --contact
```

加上 `--contact` 会多写一张 `sprites-contact.png`，方便检查抠图边缘和帧间抖动。

## 许可

[MIT](LICENSE)。角色图也随这个仓库一起提供。
