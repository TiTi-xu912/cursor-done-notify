using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Globalization;
using System.IO;
using System.Media;
using System.Text;
using System.Threading;
using System.Windows.Forms;

namespace CursorDoneBuddy
{
    public static class Program
    {
        [STAThread]
        public static int Main(string[] args)
        {
            if (args.Length > 0 && args[0] == "--selftest")
            {
                try
                {
                    SelfTest.Run();
                    Console.WriteLine("SELFTEST OK");
                    return 0;
                }
                catch (Exception ex)
                {
                    Console.WriteLine("SELFTEST FAILED: " + ex.Message);
                    return 1;
                }
            }
            BuddyForm.EnableDpi();
            if (args.Length > 0 && args[0] == "--preview")
            {
                string dir = args.Length > 1 ? args[1] : Path.Combine(StateDir(), "preview");
                Preview.Render(RootDir(), dir);
                return 0;
            }
            return HostRunner.RunSingleInstance(RootDir(), StateDir());
        }

        public static string RootDir()
        {
            return AppDomain.CurrentDomain.BaseDirectory;
        }

        public static string StateDir()
        {
            return Path.Combine(Path.GetTempPath(), "cursor-done-notify");
        }
    }

    sealed class HostRunner
    {
        readonly string _root;
        readonly string _stateDir;
        readonly string _inboxDir;
        readonly string _settingsPath = BuddySettings.DefaultPath();
        readonly DateTime _startedAt = DateTime.Now;
        BuddyState _state;
        BuddyForm _buddy;
        QuotaService _quota;
        FileSystemWatcher _watcher;
        System.Windows.Forms.Timer _timer;
        NotifyIcon _tray;
        ToolStripMenuItem _muteItem;
        ToolStripMenuItem _quietItem;
        ToolStripMenuItem _quotaItem;
        ToolStripMenuItem _themeItem;
        int _quotaSeen = -1;
        string _lastViewPhase = "";
        string _lastSignature = "";
        volatile bool _inboxDirty = true;
        DateTime _lastScan = DateTime.MinValue;

        HostRunner(string root, string stateDir)
        {
            _root = root;
            _stateDir = stateDir;
            _inboxDir = Path.Combine(stateDir, "inbox");
        }

        public static int RunSingleInstance(string root, string stateDir)
        {
            Directory.CreateDirectory(stateDir);
            using (Mutex mutex = new Mutex(false, @"Local\CursorDoneNotifyHost"))
            {
                bool owned;
                try
                {
                    owned = mutex.WaitOne(0);
                }
                catch (AbandonedMutexException)
                {
                    owned = true;
                }
                if (!owned) return 0;
                string pidFile = Path.Combine(stateDir, "host.pid");
                string pidText;
                using (Process me = Process.GetCurrentProcess())
                {
                    pidText = me.Id.ToString(CultureInfo.InvariantCulture) + "|" + me.StartTime.ToUniversalTime().Ticks.ToString(CultureInfo.InvariantCulture);
                }
                try
                {
                    WriteText(pidFile, pidText);
                    new HostRunner(root, stateDir).Run();
                }
                finally
                {
                    try
                    {
                        if (File.Exists(pidFile) && File.ReadAllText(pidFile).Trim() == pidText) File.Delete(pidFile);
                        File.Delete(Path.Combine(stateDir, "live-view.txt"));
                    }
                    catch (IOException) { }
                    catch (UnauthorizedAccessException) { }
                    mutex.ReleaseMutex();
                }
            }
            return 0;
        }

        void Run()
        {
            ClearStale();
            Application.EnableVisualStyles();
            _state = BuddyState.FromEnvironment();
            _state.Settings = BuddySettings.Load(_settingsPath);
            _state.QuotaAlertPath = Path.Combine(_stateDir, "quota-alert.txt");
            _state.Sound = PlaySound;

            _buddy = new BuddyForm();
            _buddy.SpritePath = Path.Combine(_root, @"sprites\diandian.png");
            _buddy.PositionPath = Path.Combine(_stateDir, "buddy-position.txt");
            if (Environment.GetEnvironmentVariable("CURSOR_NOTIFY_QUOTA") != "0")
            {
                _quota = new QuotaService();
                _quota.CachePath = Path.Combine(_stateDir, "quota-cache.txt");
                _quota.Start();
            }
            BuildMenu();
            ApplySettings();

            _buddy.DismissRequested += delegate { _buddy.Close(); };
            _buddy.FocusRequested += delegate { ShowSessionWindow(_state.Current); };
            _buddy.SessionActivated += delegate
            {
                Session target = _state.ClickSession(_buddy.ActivatedSessionKey);
                if (target != null) ShowSessionWindow(target);
                if (_state.Order.Count == 0)
                {
                    _buddy.Close();
                    return;
                }
                _lastSignature = "";
                Update();
            };

            Directory.CreateDirectory(_inboxDir);
            _watcher = new FileSystemWatcher(_inboxDir);
            _watcher.NotifyFilter = NotifyFilters.FileName;
            _watcher.Created += delegate { MarkInbox(); };
            _watcher.Renamed += delegate { MarkInbox(); };
            _watcher.Error += delegate { MarkInbox(); };
            _watcher.EnableRaisingEvents = true;

            _timer = new System.Windows.Forms.Timer();
            _timer.Interval = 250;
            _timer.Tick += delegate { Update(); };
            _buddy.Shown += delegate
            {
                WriteText(Path.Combine(_stateDir, "last-popup-ok.txt"), DateTime.Now.ToString("o", CultureInfo.InvariantCulture));
                _timer.Start();
            };
            _buddy.FormClosed += delegate
            {
                _timer.Stop();
                _watcher.EnableRaisingEvents = false;
                if (_quota != null) _quota.Dispose();
                _tray.Visible = false;
                _tray.Dispose();
            };

            Update();
            Place();
            Application.Run(_buddy);
            _timer.Dispose();
            _watcher.Dispose();
        }

        void MarkInbox()
        {
            _inboxDirty = true;
            try
            {
                if (_buddy != null && _buddy.IsHandleCreated && !_buddy.IsDisposed) _buddy.BeginInvoke(new MethodInvoker(Update));
            }
            catch (InvalidOperationException) { }
        }

        void Update()
        {
            if (_buddy == null || _buddy.IsDisposed) return;
            try
            {
                DateTime now = DateTime.Now;
                if (_inboxDirty || (now - _lastScan).TotalSeconds >= 2) ReadInbox();
                _state.UpdateLifecycle(now);
                _state.PlayPendingSounds(now);
                if (_state.ShouldClose(now) || (_state.Order.Count == 0 && (now - _startedAt).TotalSeconds > 60))
                {
                    _buddy.Close();
                    return;
                }
                if (_quota != null)
                {
                    int version = _quota.Version;
                    if (version != _quotaSeen)
                    {
                        _quotaSeen = version;
                        _state.UpdateTurnSpend(_quota.Latest);
                        if (_state.Settings.Quota) _state.UpdateQuotaAlert(_quota.Latest, now);
                    }
                    if (_state.NeedsSpendFollowUp(_quota.Latest, now)) _quota.RequestRefresh(true);
                    else if (_state.NeedsSpendSample()) _quota.RequestRefresh();
                }
                View view = _state.Presentation(now);
                if (_quota != null && view.Phase == "done" && _lastViewPhase != "done") _quota.RequestRefresh();
                _lastViewPhase = view.Phase;
                string notice = _state.ActiveQuotaNotice(now);
                if (notice.Length > 0)
                {
                    view.Hint = notice;
                    view.HintAlert = true;
                }
                string signature = view.Signature();
                if (signature != _lastSignature)
                {
                    _lastSignature = signature;
                    _buddy.HintAlert = view.HintAlert;
                    _buddy.CanFocusSession = _state.Order.Count <= 1 && BuddyState.CanJump(_state.Current);
                    _buddy.SetContent(view.Phase, view.Accent, view.Headline, view.Activity, view.Detail, view.Meta, view.Hint, view.History, view.Pose);
                }
                Session live = _state.Live;
                WriteText(Path.Combine(_stateDir, "live-view.txt"), string.Join("|", new string[] { live.Phase, live.Steps.ToString(CultureInfo.InvariantCulture), live.Headline, live.Activity, "" }));
            }
            catch (Exception ex)
            {
                WriteText(Path.Combine(_stateDir, "last-error.txt"), ex.ToString());
            }
        }

        void ReadInbox()
        {
            _inboxDirty = false;
            _lastScan = DateTime.Now;
            if (!Directory.Exists(_inboxDir)) return;
            string[] files = Directory.GetFiles(_inboxDir, "*.json");
            Array.Sort(files, StringComparer.Ordinal);
            foreach (string path in files)
            {
                string raw;
                try
                {
                    raw = File.ReadAllText(path, Encoding.UTF8);
                }
                catch (IOException)
                {
                    _inboxDirty = true;
                    continue;
                }
                catch (UnauthorizedAccessException)
                {
                    continue;
                }
                bool applied = string.IsNullOrWhiteSpace(raw);
                if (!applied)
                {
                    try
                    {
                        _state.Apply(HookEvent.Parse(raw));
                        applied = true;
                    }
                    catch (Exception ex)
                    {
                        WriteText(Path.Combine(_stateDir, "last-error.txt"), ex.ToString());
                    }
                }
                try
                {
                    if (applied) File.Delete(path);
                    else File.Move(path, Path.ChangeExtension(path, ".failed"));
                }
                catch (IOException) { }
                catch (UnauthorizedAccessException) { }
            }
        }

        void ClearStale()
        {
            try
            {
                if (Directory.Exists(_inboxDir))
                {
                    DateTime cutoff = DateTime.Now.AddDays(-1);
                    foreach (string path in Directory.GetFiles(_inboxDir))
                    {
                        if (File.GetLastWriteTime(path) < cutoff) File.Delete(path);
                    }
                }
                File.Delete(Path.Combine(_stateDir, "live-view.txt"));
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }

        static void PlaySound(string kind)
        {
            if (kind == "error") SystemSounds.Hand.Play();
            else if (kind == "done") SystemSounds.Asterisk.Play();
            else SystemSounds.Exclamation.Play();
        }

        void ShowSessionWindow(Session s)
        {
            if (s == null) return;
            if (BuddyState.TerminalHost(s) && DesktopWindows.ActivateHandle(s.HostWindow)) return;
            string leaf = Text.Leaf(s.WorkspaceRoot);
            if (leaf.Length == 0) return;
            if (DesktopWindows.Activate("Cursor", leaf)) return;
            try
            {
                ProcessStartInfo start = new ProcessStartInfo("cmd.exe", "/c cursor \"" + s.WorkspaceRoot + "\"");
                start.CreateNoWindow = true;
                start.UseShellExecute = false;
                Process.Start(start);
            }
            catch (Win32Exception) { }
        }

        void BuildMenu()
        {
            ContextMenuStrip menu = new ContextMenuStrip();
            _muteItem = new ToolStripMenuItem(L.T("静音", "Mute"));
            _muteItem.Click += delegate { _state.Settings.Mute = !_state.Settings.Mute; SaveAndApply(); };
            _quietItem = new ToolStripMenuItem(L.T("正看着对话时不打扰", "Stay quiet while I'm watching"));
            _quietItem.Click += delegate { _state.Settings.Quiet = !_state.Settings.Quiet; SaveAndApply(); };
            _quotaItem = new ToolStripMenuItem(L.T("显示额度栏", "Show usage bar"));
            _quotaItem.Enabled = _quota != null;
            _quotaItem.Click += delegate { _state.Settings.Quota = !_state.Settings.Quota; SaveAndApply(); };
            _themeItem = new ToolStripMenuItem(L.T("主题", "Theme"));
            string[][] themes = new string[][]
            {
                new string[] { "auto", L.T("跟随系统", "Follow Windows") },
                new string[] { "light", L.T("浅色", "Light") },
                new string[] { "dark", L.T("深色", "Dark") }
            };
            foreach (string[] theme in themes)
            {
                ToolStripMenuItem item = new ToolStripMenuItem(theme[1]);
                item.Tag = theme[0];
                item.Click += delegate(object sender, EventArgs e)
                {
                    _state.Settings.Theme = (string)((ToolStripMenuItem)sender).Tag;
                    SaveAndApply();
                };
                _themeItem.DropDownItems.Add(item);
            }
            ToolStripMenuItem usage = new ToolStripMenuItem(L.T("打开 Cursor 用量页", "Open Cursor usage page"));
            usage.Click += delegate
            {
                try
                {
                    Process.Start("https://cursor.com/dashboard?tab=usage");
                }
                catch (Win32Exception) { }
            };
            ToolStripMenuItem close = new ToolStripMenuItem(L.T("先关掉点点", "Close for now"));
            close.Click += delegate { _buddy.Close(); };
            menu.Items.Add(_muteItem);
            menu.Items.Add(_quietItem);
            menu.Items.Add(_quotaItem);
            menu.Items.Add(_themeItem);
            menu.Items.Add(usage);
            menu.Items.Add(new ToolStripSeparator());
            menu.Items.Add(close);
            _buddy.ContextMenuStrip = menu;

            _tray = new NotifyIcon();
            _tray.Icon = TrayIcon();
            _tray.Text = L.T("点点", "Diandian");
            _tray.ContextMenuStrip = menu;
            _tray.Visible = true;
        }

        void SaveAndApply()
        {
            _state.Settings.Save(_settingsPath);
            ApplySettings();
        }

        void ApplySettings()
        {
            BuddySettings settings = _state.Settings;
            _buddy.Theme = settings.Theme;
            _buddy.Quota = _quota != null && settings.Quota ? _quota : null;
            _muteItem.Checked = settings.Mute;
            _quietItem.Checked = settings.Quiet;
            _quotaItem.Checked = _quota != null && settings.Quota;
            foreach (ToolStripMenuItem item in _themeItem.DropDownItems)
            {
                item.Checked = (string)item.Tag == settings.Theme;
            }
            _lastSignature = "";
        }

        Icon TrayIcon()
        {
            string path = Path.Combine(_root, @"sprites\poses\idle-0.png");
            if (!File.Exists(path)) return SystemIcons.Application;
            using (Bitmap source = new Bitmap(path))
            using (Bitmap icon = new Bitmap(64, 64, PixelFormat.Format32bppArgb))
            {
                int side = (int)(source.Width * 0.66);
                Rectangle crop = new Rectangle((source.Width - side) / 2, (int)(source.Height * 0.02), side, side);
                using (Graphics g = Graphics.FromImage(icon))
                {
                    g.InterpolationMode = InterpolationMode.HighQualityBicubic;
                    g.DrawImage(source, new Rectangle(0, 0, 64, 64), crop, GraphicsUnit.Pixel);
                }
                return Icon.FromHandle(icon.GetHicon());
            }
        }

        void Place()
        {
            string path = _buddy.PositionPath;
            try
            {
                if (File.Exists(path))
                {
                    string[] parts = File.ReadAllText(path).Split(',');
                    int x, y;
                    if (parts.Length >= 2 && int.TryParse(parts[0].Trim(), out x) && int.TryParse(parts[1].Trim(), out y))
                    {
                        _buddy.Location = new Point(x, y);
                        _buddy.ClampToScreen();
                        return;
                    }
                }
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            Screen screen = Screen.FromPoint(Cursor.Position) ?? Screen.PrimaryScreen;
            Rectangle area = screen.WorkingArea;
            int left = Math.Max(area.Left, area.Right - _buddy.Width - 20);
            int top = Math.Max(area.Top, area.Bottom - _buddy.Height - 20);
            _buddy.Location = new Point(left, top);
        }

        static void WriteText(string path, string text)
        {
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(path));
                File.WriteAllText(path, text, new UTF8Encoding(false));
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
    }

    static class Preview
    {
        public static void Render(string root, string dir)
        {
            Directory.CreateDirectory(dir);
            using (BuddyForm form = new BuddyForm())
            {
                form.SpritePath = Path.Combine(root, @"sprites\diandian.png");
                DateTimeOffset epoch = DateTimeOffset.Now;
                string usage = "{\"billingCycleStart\":\"" + epoch.AddDays(-20).ToUnixTimeMilliseconds() + "\",\"billingCycleEnd\":\"" + epoch.AddDays(10).AddHours(-1).ToUnixTimeMilliseconds() +
                    "\",\"planUsage\":{\"totalSpend\":19235,\"includedSpend\":7000,\"bonusSpend\":12235,\"limit\":7000,\"autoPercentUsed\":13.1,\"apiPercentUsed\":32.4,\"totalPercentUsed\":14.7},\"spendLimitUsage\":{\"limitType\":\"user\"}}";
                QuotaService quota = new QuotaService();
                quota.Publish(QuotaParser.Parse(usage, "{\"planInfo\":{\"planName\":\"Pro+\"}}", "pro_plus", DateTime.Now));
                form.Quota = quota;

                string jump = "点标题跳回对话 · 点别处看步骤";
                string close = "点标题跳回对话 · 点别处关掉 · 12s";
                string steps = "已收到问题\n已完成读取 HostApp.cs\n正在思考\n已完成修改 BuddyForm.cs\n正在执行命令 tests\\hooks.ps1";
                string board = "bad|b|看报表 · 出错\nrun|a|修登录 · 正在读取 · login.cs\ndone|c|写说明 · 已完成";
                Shot[] shots = new Shot[]
                {
                    new Shot("think", "running", "blue", "正在处理", "正在思考", "", "已用 00:08 · 第 1 步", jump, "think"),
                    new Shot("read", "running", "blue", "正在处理", "正在读取", "HostApp.cs", "已用 01:12 · 第 4 步 · cursor-done-notify", jump, "read"),
                    new Shot("terminal", "running", "blue", "正在处理", "正在执行命令", "npm test", "已用 00:40 · 第 2 步", jump, "terminal"),
                    new Shot("write", "running", "blue", "正在处理", "正在修改", "BuddyForm.cs", "已用 01:40 · 第 6 步", jump, "write"),
                    new Shot("search", "running", "blue", "正在处理", "正在搜索", "pose", "已用 00:20 · 第 3 步", jump, "search"),
                    new Shot("steps", "running", "blue", "正在处理", "正在执行命令", "tests\\hooks.ps1", "已用 02:31 · 第 9 步", jump, "terminal") { History = steps, Expanded = true },
                    new Shot("done", "done", "green", "回答结束了", "cursor-done-notify", "点一下就能关掉这个小家伙。", "共用 02:04 · 11 步", close, "done"),
                    new Shot("error", "done", "red", "这一轮出错了", "可以回到对话里看", "有一步没有成功", "共用 00:40 · 3 步", close, "error"),
                    new Shot("board", "running", "red", "1 个进行中 · 1 个出错 · 1 个已完成", "", "", "共 3 个会话", "点一行只处理这一路", "error") { History = board },
                    new Shot("quota", "done", "green", "回答结束了", "cursor-done-notify", "额度栏点一下就能展开详情。", "共用 03:12 · 14 步 · 本轮约 $0.86", close, "done") { QuotaExpanded = true },
                    new Shot("alert", "running", "blue", "正在处理", "正在查找资料", "docs.cursor.com", "已用 00:52 · 第 5 步", "本期 API 额度已用 85%", "web") { HintAlert = true },
                    new Shot("dark", "done", "green", "回答结束了", "cursor-done-notify", "深色主题会跟着 Windows 设置走。", "共用 01:47 · 9 步 · 本轮约 $0.31", close, "done") { QuotaExpanded = true, Theme = "dark" }
                };
                foreach (Shot shot in shots)
                {
                    form.Theme = shot.Theme;
                    form.HintAlert = shot.HintAlert;
                    form.Expanded = shot.Expanded;
                    form.QuotaExpanded = shot.QuotaExpanded;
                    using (Bitmap bmp = form.RenderPreview(shot.Phase, shot.Accent, shot.Headline, shot.Activity, shot.Detail, shot.Meta, shot.Hint, shot.History, 0f, 0f, shot.Pose))
                    {
                        string path = Path.Combine(dir, shot.Name + ".png");
                        bmp.Save(path, ImageFormat.Png);
                        Console.WriteLine(path);
                    }
                }
            }
        }

        sealed class Shot
        {
            public string Name, Phase, Accent, Headline, Activity, Detail, Meta, Hint, Pose;
            public string History = "";
            public string Theme = "light";
            public bool Expanded;
            public bool QuotaExpanded;
            public bool HintAlert;

            public Shot(string name, string phase, string accent, string headline, string activity, string detail, string meta, string hint, string pose)
            {
                Name = name;
                Phase = phase;
                Accent = accent;
                Headline = headline;
                Activity = activity;
                Detail = detail;
                Meta = meta;
                Hint = hint;
                Pose = pose;
            }
        }
    }

    static class SelfTest
    {
        static void Check(bool ok, string message)
        {
            if (!ok) throw new InvalidOperationException(message);
        }

        static void Phase(BuddyState st, string name, string expected)
        {
            Session s = st.Live;
            Check(s.Phase == expected, name + " expected " + expected + " but was " + s.Phase + " (stop=" + s.StopSeen + " answer=" + s.AnswerSeen + " tools=" + s.OpenTools + ")");
        }

        static BuddyState Fresh()
        {
            BuddyState st = new BuddyState();
            st.Sound = delegate { };
            st.Foreground = delegate { return new string[] { "", "", "0" }; };
            return st;
        }

        static HookEvent Ev(params string[] pairs)
        {
            Dictionary<string, object> map = new Dictionary<string, object>();
            for (int i = 0; i + 1 < pairs.Length; i += 2)
            {
                if (pairs[i] == "workspace_roots") map[pairs[i]] = new object[] { pairs[i + 1] };
                else map[pairs[i]] = pairs[i + 1];
            }
            return HookEvent.FromMap(map);
        }

        public static void Run()
        {
            bool english = L.English;
            L.English = false;
            try
            {
                Lifecycle();
                ToolIds();
                Generations();
                MultiSession();
                Poses();
                Spend();
                Alerts();
                Focus();
                Parsing();
            }
            finally
            {
                L.English = english;
            }
        }

        static void Lifecycle()
        {
            BuddyState st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "generation_id", "g1", "prompt", "你好", "workspace_roots", @"D:\demo"));
            Phase(st, "prompt", "running");
            st.Apply(Ev("hook_event_name", "preToolUse", "generation_id", "g1", "tool_name", "Read", "file_path", "a.txt"));
            Phase(st, "tool", "running");
            Check(st.Live.OpenTools == 1, "open tools");
            st.Apply(Ev("hook_event_name", "postToolUse", "generation_id", "g1", "tool_name", "Read", "file_path", "a.txt"));
            Phase(st, "tool done", "running");
            Check(st.Live.OpenTools == 0, "tools not closed");
            st.Apply(Ev("hook_event_name", "afterAgentResponse", "generation_id", "g1", "text", "已经好了"));
            Phase(st, "answer", "settling");
            st.Apply(Ev("hook_event_name", "afterAgentThought", "generation_id", "g1", "text", "late"));
            Phase(st, "late thought", "settling");
            st.Apply(Ev("hook_event_name", "stop", "generation_id", "g1", "status", "completed"));
            Phase(st, "stop", "done");
            st.Apply(Ev("hook_event_name", "afterAgentResponse", "generation_id", "g1", "text", "late answer"));
            Phase(st, "late answer", "done");
            st.Apply(Ev("hook_event_name", "preToolUse", "generation_id", "g1", "tool_name", "Shell"));
            Phase(st, "late tool", "done");

            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "generation_id", "g2", "prompt", "下一问"));
            Phase(st, "new turn", "running");
            st.Apply(Ev("hook_event_name", "preToolUse", "generation_id", "g2", "tool_name", "Read"));
            st.Apply(Ev("hook_event_name", "afterAgentResponse", "generation_id", "g2", "text", "先说一句"));
            Phase(st, "answer while tool open", "running");
            st.Apply(Ev("hook_event_name", "postToolUse", "generation_id", "g2", "tool_name", "Read"));
            Phase(st, "tool closed after answer", "settling");
            st.Apply(Ev("hook_event_name", "preToolUse", "generation_id", "g2", "tool_name", "Write"));
            Phase(st, "work resumed", "running");
            st.Apply(Ev("hook_event_name", "postToolUse", "generation_id", "g2", "tool_name", "Write"));
            st.Apply(Ev("hook_event_name", "afterAgentResponse", "generation_id", "g2", "text", "最终回复"));
            Phase(st, "final answer", "settling");
            st.Live.SettleUntil = DateTime.Now.AddSeconds(-1);
            st.UpdateLifecycle(DateTime.Now);
            Phase(st, "soft done", "done");
            Check(!st.Live.StopSeen, "soft done must not lock the generation");
            st.Apply(Ev("hook_event_name", "preToolUse", "generation_id", "g2", "tool_name", "Grep"));
            Phase(st, "resume after soft done", "running");
            Check(!st.Live.SoundPending, "resumed work kept a pending completion sound");

            st = Fresh();
            st.Apply(Ev("hook_event_name", "stop", "generation_id", "g3", "status", "completed"));
            st.Apply(Ev("hook_event_name", "afterAgentResponse", "generation_id", "g3", "text", "out of order"));
            Phase(st, "stop before response", "done");

            st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "stale", "generation_id", "stale-g", "prompt", "失联测试"));
            st.Live.UpdatedAt = DateTime.Now.AddMinutes(-(st.StaleMinutes + 1));
            st.UpdateLifecycle(DateTime.Now);
            Phase(st, "stale session", "done");
            Check(st.Live.Headline == "会话失去连接" && !st.Live.SoundPending, "stale session did not expire quietly");
            Check(st.CurrentPose(st.Live, DateTime.Now) == "sleep", "a stale session dozes");
        }

        static void ToolIds()
        {
            BuddyState st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "generation_id", "tool-ids", "prompt", "工具去重"));
            st.Apply(Ev("hook_event_name", "preToolUse", "generation_id", "tool-ids", "tool_name", "Read", "tool_use_id", "t1"));
            st.Apply(Ev("hook_event_name", "preToolUse", "generation_id", "tool-ids", "tool_name", "Read", "tool_use_id", "t1"));
            Check(st.Live.OpenTools == 1 && st.Live.Steps == 1, "duplicate tool start was counted twice");
            st.Apply(Ev("hook_event_name", "postToolUse", "generation_id", "tool-ids", "tool_name", "Read", "tool_use_id", "t1"));
            st.Apply(Ev("hook_event_name", "postToolUse", "generation_id", "tool-ids", "tool_name", "Read", "tool_use_id", "t1"));
            Check(st.Live.OpenTools == 0 && st.Live.Steps == 1, "duplicate tool stop changed tool state");

            st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "late-pre", "generation_id", "late-pre-g", "prompt", "迟到事件"));
            st.Apply(Ev("hook_event_name", "preToolUse", "conversation_id", "late-pre", "generation_id", "late-pre-g", "tool_name", "Read", "tool_use_id", "late-t1"));
            st.Apply(Ev("hook_event_name", "afterAgentResponse", "conversation_id", "late-pre", "generation_id", "late-pre-g", "text", "回答已写好"));
            st.Apply(Ev("hook_event_name", "preToolUse", "conversation_id", "late-pre", "generation_id", "late-pre-g", "tool_name", "Read", "tool_use_id", "late-t1"));
            Check(st.Live.AnswerSeen, "duplicate tool start cleared a completed answer");
            st.Apply(Ev("hook_event_name", "postToolUse", "conversation_id", "late-pre", "generation_id", "late-pre-g", "tool_name", "Read", "tool_use_id", "late-t1"));
            Phase(st, "duplicate start after answer", "settling");

            st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "safe", "generation_id", "safe-g", "prompt", "安全模式"));
            st.Apply(Ev("hook_event_name", "postToolUseFailure", "conversation_id", "safe", "generation_id", "safe-g", "tool_name", "Shell", "tool_use_id", "failed", "error_message", "失败"));
            Check(st.Live.Accent == "orange" && st.Live.Steps == 1, "post-only failure was not counted");
            st.Apply(Ev("hook_event_name", "postToolUse", "conversation_id", "safe", "generation_id", "safe-g", "tool_name", "Read", "tool_use_id", "success", "file_path", "safe.txt"));
            Phase(st, "post-only success", "running");
            Check(st.Live.Accent == "blue" && st.Live.Steps == 2 && st.Live.Pose == "read", "post-only success kept stale state");
            Check(string.Join("\n", st.Live.History.ToArray()).Contains("已完成读取"), "post-only success was not added to history");
            st.Apply(Ev("hook_event_name", "postToolUse", "conversation_id", "safe", "generation_id", "safe-g", "tool_name", "Read", "tool_use_id", "success", "file_path", "safe.txt"));
            Check(st.Live.Steps == 2, "duplicate post-only success was counted twice");
        }

        static void Generations()
        {
            BuddyState st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "gen-chat", "generation_id", "old-gen", "prompt", "旧轮次"));
            st.Apply(Ev("hook_event_name", "stop", "conversation_id", "gen-chat", "generation_id", "old-gen", "status", "completed"));
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "gen-chat", "generation_id", "new-gen", "prompt", "新轮次"));
            st.Apply(Ev("hook_event_name", "postToolUse", "conversation_id", "gen-chat", "generation_id", "old-gen", "tool_name", "Read"));
            Phase(st, "old generation ignored", "running");
            Check(st.Live.GenerationId == "new-gen", "old generation replaced active generation");
        }

        static void MultiSession()
        {
            BuddyState st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "chat-a", "generation_id", "ga", "prompt", "修登录"));
            st.Apply(Ev("hook_event_name", "preToolUse", "conversation_id", "chat-a", "generation_id", "ga", "tool_name", "Read", "file_path", "login.cs"));
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "chat-b", "generation_id", "gb", "prompt", "看报表"));
            st.Apply(Ev("hook_event_name", "stop", "conversation_id", "chat-b", "generation_id", "gb", "status", "error"));
            Phase(st, "second chat failed", "done");
            Check(st.Live.Accent == "red", "second chat should be the error");
            st.Apply(Ev("hook_event_name", "postToolUse", "conversation_id", "chat-a", "generation_id", "ga", "tool_name", "Read", "file_path", "login.cs"));
            Phase(st, "first chat still running", "running");
            Check(st.Sessions["chat-b"].Phase == "done" && st.Sessions["chat-b"].Accent == "red", "finished chat lost its error");
            View overview = st.Presentation(DateTime.Now);
            Check(overview.Phase == "running" && overview.Accent == "red", "overview with a running chat must stay running: " + overview.Phase + "/" + overview.Accent);
            string board = st.BoardText();
            Check(board.Contains("bad|") && board.Contains("run|"), "board missing a row: " + board);
            st.ClickSession("chat-b");
            Check(!st.Sessions.ContainsKey("chat-b"), "click should drop the finished chat");
            Check(st.Sessions["chat-a"].Phase == "running", "click changed the other chat");
            Check(st.ClickSession("chat-a") != null && st.PinnedKey == "chat-a", "click should pin the running chat");
            st.ClickSession("chat-a");
            Check(st.PinnedKey.Length == 0, "second click should return to the overview");
        }

        static void Poses()
        {
            BuddyState st = Fresh();
            DateTime now = DateTime.Now;
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "poses", "generation_id", "poses-g", "prompt", "动作"));
            st.Apply(Ev("hook_event_name", "postToolUse", "conversation_id", "poses", "generation_id", "poses-g", "tool_name", "Task", "tool_use_id", "p1"));
            Check(st.CurrentPose(st.Live, now) == "delegate", "Task pose");
            st.Apply(Ev("hook_event_name", "postToolUse", "conversation_id", "poses", "generation_id", "poses-g", "tool_name", "WebFetch", "tool_use_id", "p2"));
            Check(st.CurrentPose(st.Live, now) == "web", "WebFetch pose");
            st.Live.StepStartedAt = now.AddSeconds(-(st.DozeSeconds + 5));
            Check(st.CurrentPose(st.Live, now) == "sleep", "a quiet turn should doze");
            st.Live.OpenTools = 1;
            Check(st.CurrentPose(st.Live, now) != "sleep", "an open tool must not doze");
            st.Live.OpenTools = 0;
            st.Apply(Ev("hook_event_name", "stop", "conversation_id", "poses", "generation_id", "poses-g", "status", "aborted"));
            Check(st.CurrentPose(st.Live, now) == "stop", "aborted pose");
        }

        static void Spend()
        {
            BuddyState st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "spend", "generation_id", "spend-g", "prompt", "花费"));
            st.UpdateTurnSpend(new QuotaSnapshot { Ok = true, TotalSpendCents = 1000, FetchedAt = DateTime.Now });
            Check(st.Live.SpendStart == 1000, "spend baseline");
            Check(!st.NeedsSpendSample(), "baseline should be satisfied");
            st.UpdateTurnSpend(new QuotaSnapshot { Ok = true, TotalSpendCents = 1010, FetchedAt = DateTime.Now });
            Check(st.Live.SpendStart == 1000, "baseline must not move while running");
            st.Apply(Ev("hook_event_name", "stop", "conversation_id", "spend", "generation_id", "spend-g", "status", "completed"));
            st.UpdateTurnSpend(new QuotaSnapshot { Ok = true, TotalSpendCents = 1042, FetchedAt = DateTime.Now.AddSeconds(1) });
            Check(st.MetaText(st.Live, DateTime.Now).Contains("本轮约 $0.42"), "turn spend: " + st.MetaText(st.Live, DateTime.Now));
            Check(st.NeedsSpendFollowUp(new QuotaSnapshot { FetchedAt = DateTime.Now.AddSeconds(1) }, DateTime.Now), "a fresh finish should keep sampling spend");
            Check(!st.NeedsSpendFollowUp(new QuotaSnapshot { FetchedAt = DateTime.Now.AddSeconds(6) }, DateTime.Now), "a late enough sample should stop the follow-up");
            st.Live.DoneAt = DateTime.Now.AddSeconds(-20);
            Check(!st.NeedsSpendFollowUp(null, DateTime.Now), "follow-up must give up after 15 seconds");

            st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "stale-baseline", "generation_id", "sb", "prompt", "旧数据"));
            st.UpdateTurnSpend(new QuotaSnapshot { Ok = true, TotalSpendCents = 500, FetchedAt = DateTime.Now.AddMinutes(-10) });
            Check(st.Live.SpendStart < 0, "an old cached snapshot must not become the baseline");
        }

        static void Alerts()
        {
            BuddyState st = Fresh();
            int sounds = 0;
            st.Sound = delegate { sounds++; };
            st.QuotaAlertPath = Path.Combine(Path.GetTempPath(), "cursor-done-notify-alert-" + Guid.NewGuid().ToString("n") + ".txt");
            try
            {
                DateTime now = DateTime.Now;
                DateTime cycleEnd = now.AddDays(9);
                st.UpdateQuotaAlert(new QuotaSnapshot { Ok = true, TotalPercent = 60, ApiPercent = 20, CycleEnd = cycleEnd }, now);
                Check(st.ActiveQuotaNotice(now).Length == 0, "no alert below 80%");
                st.UpdateQuotaAlert(new QuotaSnapshot { Ok = true, TotalPercent = 61, ApiPercent = 85.6, CycleEnd = cycleEnd }, now);
                Check(st.ActiveQuotaNotice(now) == "本期 API 额度已用 85%", "80% alert: " + st.ActiveQuotaNotice(now));
                Check(sounds == 1, "the alert should sound once");
                st.QuotaNotice = "";
                st.UpdateQuotaAlert(new QuotaSnapshot { Ok = true, TotalPercent = 62, ApiPercent = 88, CycleEnd = cycleEnd }, now);
                Check(st.ActiveQuotaNotice(now).Length == 0, "the same level must alert only once per cycle");
                st.UpdateQuotaAlert(new QuotaSnapshot { Ok = true, TotalPercent = 101, ApiPercent = 90, CycleEnd = cycleEnd }, now);
                Check(st.ActiveQuotaNotice(now) == "本期额度已用完", "100% alert: " + st.ActiveQuotaNotice(now));
                st.QuotaNotice = "";
                st.UpdateQuotaAlert(new QuotaSnapshot { Ok = true, TotalPercent = 85, ApiPercent = 10, CycleEnd = cycleEnd.AddDays(30) }, now);
                Check(st.ActiveQuotaNotice(now).Length > 0, "a new billing cycle should alert again");
                Check(st.ActiveQuotaNotice(now.AddSeconds(20)).Length == 0, "the notice should fade");
            }
            finally
            {
                File.Delete(st.QuotaAlertPath);
            }
        }

        static void Focus()
        {
            BuddyState st = Fresh();
            int sounds = 0;
            st.Sound = delegate { sounds++; };
            st.Foreground = delegate { return new string[] { "Cursor", "login.cs - demo - Cursor", "42" }; };
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "quiet", "generation_id", "quiet-g", "prompt", "安静", "workspace_roots", @"D:\work\demo"));
            Check(st.Hint(st.Live, DateTime.Now).Contains("点标题跳回对话"), "hint should offer the jump");
            st.Apply(Ev("hook_event_name", "stop", "conversation_id", "quiet", "generation_id", "quiet-g", "status", "completed"));
            st.PlayPendingSounds(DateTime.Now);
            Check(!st.Live.SoundPending && st.Live.SoundPlayed, "a quiet finish should still settle the sound flag");
            Check(sounds == 0, "a focused finish must not make a sound");
            Check((st.Live.DoneUntil.Value - DateTime.Now).TotalSeconds <= 5, "a focused finish should leave quickly");
            st.Foreground = delegate { return new string[] { "Cursor", "other.cs - another-project - Cursor", "43" }; };
            Check(!st.SessionFocused(st.Live), "another Cursor window is not this session");

            st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "cli", "generation_id", "cli-g", "prompt", "终端", "host_window", "134608", "host_process", "WindowsTerminal"));
            Session s = st.Sessions["cli"];
            Check(s.HostWindow == 134608 && s.HostProcess == "WindowsTerminal", "host window was not recorded");
            Check(BuddyState.CanJump(s), "a terminal session without workspace roots can still jump");
            st.Foreground = delegate { return new string[] { "WindowsTerminal", "Some chat title", "134608" }; };
            Check(st.SessionFocused(s), "the terminal running the agent counts as focused");
            st.Foreground = delegate { return new string[] { "WindowsTerminal", "Some chat title", "999" }; };
            Check(!st.SessionFocused(s), "another terminal window is not this session");

            st = Fresh();
            st.Apply(Ev("hook_event_name", "beforeSubmitPrompt", "conversation_id", "loud", "generation_id", "loud-g", "prompt", "提醒"));
            st.Apply(Ev("hook_event_name", "stop", "conversation_id", "loud", "generation_id", "loud-g", "status", "error"));
            string played = "";
            st.Sound = delegate(string kind) { played = kind; };
            st.PlayPendingSounds(DateTime.Now);
            Check(played == "error", "an unwatched failure should sound: " + played);
        }

        static void Parsing()
        {
            HookEvent parsed = HookEvent.Parse("{\"hook_event_name\":\"preToolUse\",\"workspace_roots\":[\"D:\\\\a\\\\proj\",\"E:\\\\b\"],\"tool_input\":{\"file_path\":\"D:\\\\a\\\\proj\\\\src\\\\main.cs\"},\"tool_name\":\"Read\"}");
            Check(parsed.WorkspaceRoots().Count == 2, "workspace roots");
            BuddyState st = Fresh();
            st.Apply(parsed);
            Check(st.Live.Project == "proj\u3001b", "project name: " + st.Live.Project);
            Check(st.Live.Detail == "main.cs", "tool detail from tool_input: " + st.Live.Detail);

            HookEvent truncated = HookEvent.Parse("{\"hook_event_name\":\"stop\",\"conversation_id\":\"cut\",\"status\":\"error\",\"text\":\"unterminated");
            Check(truncated.Get("hook_event_name") == "stop" && truncated.Get("status") == "error", "regex fallback for a cut-off body");

            Check(Text.Elapsed(DateTime.Now.AddMinutes(-59).AddSeconds(-40), DateTime.Now) == "59:40", "elapsed minutes must not round up");
            Check(Text.Elapsed(DateTime.Now.AddMinutes(-100), DateTime.Now).StartsWith("1:40:"), "elapsed hours must not round up");
        }
    }
}
