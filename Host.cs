using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;

namespace CursorDoneBuddy
{
    public static class L
    {
        public static bool English;

        public static string T(string zh, string en)
        {
            return English ? en : zh;
        }
    }

    static class Text
    {
        public static string Shorten(string text, int max)
        {
            if (string.IsNullOrWhiteSpace(text)) return "";
            string clean = Regex.Replace(text, @"\s+", " ").Trim();
            if (clean.Length <= max) return clean;
            return clean.Substring(0, max) + "\u2026";
        }

        public static string Elapsed(DateTime? start, DateTime now)
        {
            if (!start.HasValue) return "00:00";
            TimeSpan span = now - start.Value;
            if (span < TimeSpan.Zero) span = TimeSpan.Zero;
            if (span.TotalHours >= 1)
            {
                return string.Format(CultureInfo.InvariantCulture, "{0}:{1:d2}:{2:d2}", (int)Math.Floor(span.TotalHours), span.Minutes, span.Seconds);
            }
            return string.Format(CultureInfo.InvariantCulture, "{0:d2}:{1:d2}", (int)Math.Floor(span.TotalMinutes), span.Seconds);
        }

        public static string Money(long cents)
        {
            return "$" + (cents / 100.0).ToString("0.00", CultureInfo.InvariantCulture);
        }

        public static string Leaf(string path)
        {
            if (string.IsNullOrWhiteSpace(path)) return "";
            string trimmed = path.TrimEnd('\\', '/');
            int cut = Math.Max(trimmed.LastIndexOf('\\'), trimmed.LastIndexOf('/'));
            string leaf = cut >= 0 ? trimmed.Substring(cut + 1) : trimmed;
            return string.IsNullOrEmpty(leaf) ? path : leaf;
        }
    }

    public sealed class HookEvent
    {
        readonly IDictionary _map;
        readonly string _raw;

        HookEvent(IDictionary map, string raw)
        {
            _map = map;
            _raw = raw ?? "";
        }

        public static HookEvent Parse(string raw)
        {
            IDictionary map = null;
            try
            {
                JavaScriptSerializer json = new JavaScriptSerializer();
                json.MaxJsonLength = int.MaxValue;
                map = json.DeserializeObject(raw ?? "") as IDictionary;
            }
            catch (ArgumentException) { }
            catch (InvalidOperationException) { }
            return new HookEvent(map, raw);
        }

        public static HookEvent FromMap(IDictionary map)
        {
            return new HookEvent(map, "");
        }

        public string Get(string name)
        {
            if (_map != null)
            {
                object value = _map[name];
                if (value == null || value is IDictionary || value is object[]) return "";
                return Convert.ToString(value, CultureInfo.InvariantCulture);
            }
            Match match = Regex.Match(_raw, "\"" + Regex.Escape(name) + "\"\\s*:\\s*\"((?:\\\\.|[^\"\\\\])*)\"");
            if (!match.Success) return "";
            try
            {
                return new JavaScriptSerializer().Deserialize<string>("\"" + match.Groups[1].Value + "\"") ?? "";
            }
            catch (ArgumentException)
            {
                return match.Groups[1].Value;
            }
        }

        public object Value(string name)
        {
            return _map == null ? null : _map[name];
        }

        public List<string> WorkspaceRoots()
        {
            List<string> roots = new List<string>();
            object[] list = Value("workspace_roots") as object[];
            if (list == null)
            {
                ArrayList array = Value("workspace_roots") as ArrayList;
                if (array != null) list = array.ToArray();
            }
            if (list == null) return roots;
            foreach (object item in list)
            {
                string text = Convert.ToString(item, CultureInfo.InvariantCulture);
                if (!string.IsNullOrWhiteSpace(text)) roots.Add(text);
            }
            return roots;
        }
    }

    public sealed class Session
    {
        public string Key = "";
        public string Phase = "idle";
        public string Headline = L.T("点点在这儿", "Diandian is here");
        public string Activity = L.T("等你开始提问", "Waiting for your question");
        public string Pose = "idle";
        public string Detail = "";
        public string Prompt = "";
        public string Project = "";
        public string Snippet = "";
        public string Accent = "blue";
        public string Summary = "";
        public string GenerationId = "";
        public string WorkspaceRoot = "";
        public string HostProcess = "";
        public long HostWindow;
        public DateTime? StartedAt;
        public DateTime? StepStartedAt;
        public DateTime? DoneUntil;
        public DateTime? SettleUntil;
        public DateTime? DoneAt;
        public DateTime UpdatedAt = DateTime.Now;
        public int Steps;
        public int OpenTools;
        public int OpenSubagents;
        public bool SoundPlayed;
        public bool SoundPending;
        public bool StopSeen;
        public bool AnswerSeen;
        public bool SpendShared;
        public long SpendStart = -1;
        public long SpendEnd = -1;
        public List<string> History = new List<string>();
        public HashSet<string> OpenToolIds = new HashSet<string>();
        public HashSet<string> CompletedToolIds = new HashSet<string>();
        public List<string> RetiredGenerations = new List<string>();

        public bool WorkOutstanding
        {
            get { return OpenTools > 0 || OpenSubagents > 0; }
        }
    }

    public sealed class View
    {
        public string Phase = "";
        public string Accent = "";
        public string Headline = "";
        public string Activity = "";
        public string Detail = "";
        public string Meta = "";
        public string Hint = "";
        public string Pose = "";
        public string History = "";
        public bool HintAlert;

        public string Signature()
        {
            return string.Join("\n", new string[] { HintAlert ? "1" : "0", Phase, Accent, Headline, Activity, Detail, Meta, Hint, Pose, History });
        }
    }

    public sealed class BuddySettings
    {
        public bool Mute;
        public bool Quota = true;
        public bool Quiet = true;
        public string Theme = "auto";

        public static string DefaultPath()
        {
            return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), @"cursor-done-notify\settings.txt");
        }

        public static BuddySettings Load(string path)
        {
            BuddySettings settings = new BuddySettings();
            try
            {
                if (string.IsNullOrEmpty(path) || !File.Exists(path)) return settings;
                foreach (string line in File.ReadAllText(path, Encoding.UTF8).Split('\n'))
                {
                    int eq = line.IndexOf('=');
                    if (eq <= 0) continue;
                    string key = line.Substring(0, eq).Trim();
                    string value = line.Substring(eq + 1).Trim();
                    switch (key)
                    {
                        case "mute": settings.Mute = value == "1"; break;
                        case "quota": settings.Quota = value != "0"; break;
                        case "quiet": settings.Quiet = value != "0"; break;
                        case "theme": if (value == "light" || value == "dark") settings.Theme = value; break;
                    }
                }
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            return settings;
        }

        public void Save(string path)
        {
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(path));
                string text = "mute=" + (Mute ? "1" : "0") + "\nquota=" + (Quota ? "1" : "0") + "\nquiet=" + (Quiet ? "1" : "0") + "\ntheme=" + Theme + "\n";
                File.WriteAllText(path, text, new UTF8Encoding(false));
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
    }

    public sealed class BuddyState
    {
        public int DoneSeconds = 12;
        public int StaleMinutes = 180;
        public int DozeSeconds = 90;
        public BuddySettings Settings = new BuddySettings();
        public string QuotaAlertPath = "";
        public string QuotaNotice = "";
        public DateTime? QuotaNoticeUntil;
        public string PinnedKey = "";
        public Func<string[]> Foreground = DesktopWindows.Foreground;
        public Action<string> Sound = delegate { };

        public readonly Dictionary<string, Session> Sessions = new Dictionary<string, Session>();
        public readonly List<string> Order = new List<string>();
        public Session Current;
        readonly Session _idle = new Session();

        public Session Live
        {
            get { return Current ?? _idle; }
        }

        public static BuddyState FromEnvironment()
        {
            BuddyState state = new BuddyState();
            state.DoneSeconds = EnvInt("CURSOR_NOTIFY_SECONDS", 12, 3, 60);
            state.StaleMinutes = EnvInt("CURSOR_NOTIFY_STALE_MINUTES", 180, 10, 1440);
            return state;
        }

        static int EnvInt(string name, int fallback, int min, int max)
        {
            string text = Environment.GetEnvironmentVariable(name);
            int value;
            if (string.IsNullOrEmpty(text) || !Regex.IsMatch(text, @"^\d+$") || !int.TryParse(text, out value)) return fallback;
            return Math.Max(min, Math.Min(max, value));
        }

        // ---- sessions ----

        Session Use(string key)
        {
            if (string.IsNullOrWhiteSpace(key)) key = "default";
            Session session;
            if (!Sessions.TryGetValue(key, out session))
            {
                session = new Session();
                session.Key = key;
                Sessions[key] = session;
                Order.Add(key);
            }
            Current = session;
            return session;
        }

        public void RemoveSession(string key)
        {
            Order.Remove(key);
            Sessions.Remove(key);
            if (PinnedKey == key) PinnedKey = "";
            if (Current != null && Current.Key == key)
            {
                Current = Order.Count > 0 ? Sessions[Order[Order.Count - 1]] : null;
            }
        }

        void TrimSessions()
        {
            while (Order.Count > 8)
            {
                string victim = null;
                foreach (string key in Order)
                {
                    string phase = Sessions[key].Phase;
                    if (phase == "done" || phase == "idle") { victim = key; break; }
                }
                if (victim == null) break;
                Order.Remove(victim);
                Sessions.Remove(victim);
                if (Current != null && Current.Key == victim) Current = null;
            }
            while (Order.Count > 16)
            {
                string oldest = null;
                DateTime oldestAt = DateTime.MaxValue;
                foreach (string key in Order)
                {
                    if (Sessions[key].UpdatedAt < oldestAt) { oldestAt = Sessions[key].UpdatedAt; oldest = key; }
                }
                if (oldest == null) break;
                RemoveSession(oldest);
            }
        }

        bool OtherSessionRunning(Session self)
        {
            foreach (Session other in Sessions.Values)
            {
                if (other == self) continue;
                if (other.Phase == "running" || other.Phase == "settling") return true;
            }
            return false;
        }

        // ---- event handling ----

        static string ConversationId(HookEvent e)
        {
            string id = e.Get("conversation_id");
            if (string.IsNullOrWhiteSpace(id)) id = e.Get("generation_id");
            return string.IsNullOrWhiteSpace(id) ? "default" : id;
        }

        static void AddRetired(Session s, string generation)
        {
            if (string.IsNullOrWhiteSpace(generation)) return;
            if (!s.RetiredGenerations.Contains(generation)) s.RetiredGenerations.Add(generation);
            while (s.RetiredGenerations.Count > 16) s.RetiredGenerations.RemoveAt(0);
        }

        static void AddHistory(Session s, string line)
        {
            line = Text.Shorten(line, 72);
            if (string.IsNullOrWhiteSpace(line)) return;
            if (s.History.Count > 0 && s.History[s.History.Count - 1] == line) return;
            s.History.Add(line);
            while (s.History.Count > 5) s.History.RemoveAt(0);
        }

        void ResetTurn(Session s, DateTime now)
        {
            s.Phase = "running";
            s.StartedAt = now;
            s.StepStartedAt = now;
            s.Steps = 0;
            s.History = new List<string>();
            s.Snippet = "";
            s.DoneUntil = null;
            s.SettleUntil = null;
            s.SoundPlayed = false;
            s.SoundPending = false;
            s.StopSeen = false;
            s.AnswerSeen = false;
            s.OpenTools = 0;
            s.OpenSubagents = 0;
            s.OpenToolIds = new HashSet<string>();
            s.CompletedToolIds = new HashSet<string>();
            s.Accent = "blue";
            s.Summary = "";
            s.Headline = L.T("正在处理", "Working");
            s.Activity = "";
            s.Detail = "";
            s.Pose = "think";
            s.SpendStart = -1;
            s.SpendEnd = -1;
            s.SpendShared = OtherSessionRunning(s);
            s.DoneAt = null;
        }

        static void EnterRunning(Session s)
        {
            if (s.StopSeen) return;
            s.SoundPlayed = false;
            s.SoundPending = false;
            s.Phase = "running";
            s.SettleUntil = null;
            s.DoneUntil = null;
            s.DoneAt = null;
            s.SpendEnd = -1;
            s.Headline = L.T("正在处理", "Working");
            s.Accent = "blue";
        }

        static void HoldAnswer(Session s, DateTime now)
        {
            if (s.StopSeen || s.WorkOutstanding) return;
            s.Phase = "settling";
            s.DoneUntil = null;
            s.Headline = L.T("回复写好了", "Reply is ready");
            s.Accent = "green";
            s.Activity = L.T("可以先看对话", "You can read it now");
            s.Pose = "done";
            if (!string.IsNullOrEmpty(s.Snippet)) s.Detail = s.Snippet;
            s.SettleUntil = now.AddSeconds(8);
            if (s.StartedAt.HasValue)
            {
                s.Summary = L.T("已用 ", "") + Text.Elapsed(s.StartedAt, now) + L.T(" · " + s.Steps + " 步", " · " + s.Steps + " steps");
            }
            AddHistory(s, L.T("回复写好了", "Reply is ready"));
        }

        static void UpdateAnswerState(Session s, DateTime now)
        {
            if (s.AnswerSeen) HoldAnswer(s, now);
        }

        string FinishedSummary(Session s, DateTime now)
        {
            return L.T("共用 ", "Took ") + Text.Elapsed(s.StartedAt, now) + L.T(" · " + s.Steps + " 步", " · " + s.Steps + " steps");
        }

        void CompleteTurn(Session s, string status, DateTime now)
        {
            bool firstDone = s.Phase != "done";
            s.StopSeen = true;
            s.Phase = "done";
            s.SettleUntil = null;
            s.OpenTools = 0;
            s.OpenSubagents = 0;
            s.OpenToolIds = new HashSet<string>();
            if (status == "aborted")
            {
                s.Headline = L.T("这一轮已中断", "Turn stopped");
                s.Accent = "orange";
                s.Pose = "stop";
            }
            else if (status == "error")
            {
                s.Headline = L.T("这一轮出错了", "Turn failed");
                s.Accent = "red";
                s.Pose = "error";
            }
            else
            {
                s.Headline = L.T("回答结束了", "All done");
                s.Accent = "green";
                s.Pose = "done";
            }
            s.DoneAt = now;
            if (OtherSessionRunning(s)) s.SpendShared = true;
            s.Summary = FinishedSummary(s, now);
            s.Activity = !string.IsNullOrEmpty(s.Project) ? s.Project : L.T("可以回到对话里看", "Back to the chat for details");
            if (!string.IsNullOrEmpty(s.Snippet)) s.Detail = s.Snippet;
            if (firstDone)
            {
                s.DoneUntil = now.AddSeconds(DoneSeconds);
                s.SoundPending = true;
            }
            AddRetired(s, s.GenerationId);
            AddHistory(s, s.Headline);
        }

        void ExpireStaleTurn(Session s, DateTime now)
        {
            if (s.Phase == "done" || s.Phase == "idle") return;
            s.StopSeen = true;
            s.Phase = "done";
            s.SettleUntil = null;
            s.OpenTools = 0;
            s.OpenSubagents = 0;
            s.OpenToolIds = new HashSet<string>();
            s.Headline = L.T("会话失去连接", "Session went quiet");
            s.Activity = L.T("长时间没有收到 Cursor 事件", "No Cursor events for a long time");
            s.Detail = "";
            s.Accent = "orange";
            s.Pose = "sleep";
            s.Summary = L.T("最后活动于 ", "Last activity ") + s.UpdatedAt.ToString("HH:mm", CultureInfo.InvariantCulture);
            s.DoneUntil = now.AddSeconds(DoneSeconds);
            s.SoundPending = false;
            AddRetired(s, s.GenerationId);
            AddHistory(s, s.Headline);
        }

        void SoftComplete(Session s, DateTime now)
        {
            if (s.StopSeen || s.Phase == "done") return;
            s.Phase = "done";
            s.SettleUntil = null;
            s.Headline = L.T("回答结束了", "All done");
            s.Accent = "green";
            s.Pose = "done";
            s.Summary = FinishedSummary(s, now);
            s.DoneAt = now;
            if (OtherSessionRunning(s)) s.SpendShared = true;
            if (!string.IsNullOrEmpty(s.Project)) s.Activity = s.Project;
            else if (string.IsNullOrWhiteSpace(s.Activity)) s.Activity = L.T("可以回到对话里看", "Back to the chat for details");
            if (!string.IsNullOrEmpty(s.Snippet)) s.Detail = s.Snippet;
            s.DoneUntil = now.AddSeconds(DoneSeconds);
            s.SoundPending = true;
            AddHistory(s, s.Headline);
        }

        public void Apply(HookEvent e)
        {
            Apply(e, DateTime.Now);
        }

        public void Apply(HookEvent e, DateTime now)
        {
            string eventName = e.Get("hook_event_name");
            if (string.IsNullOrWhiteSpace(eventName)) return;
            Session s = Use(ConversationId(e));
            try
            {
                ApplyTo(s, e, eventName, now);
            }
            finally
            {
                s.UpdatedAt = now;
                TrimSessions();
            }
        }

        void ApplyTo(Session s, HookEvent e, string eventName, DateTime now)
        {
            List<string> roots = e.WorkspaceRoots();
            if (roots.Count > 0)
            {
                List<string> names = new List<string>();
                foreach (string root in roots) names.Add(Text.Leaf(root));
                s.Project = string.Join("\u3001", names.ToArray());
                s.WorkspaceRoot = roots[0];
            }

            string gen = e.Get("generation_id");
            if (!string.IsNullOrWhiteSpace(gen) && s.RetiredGenerations.Contains(gen)) return;
            if (eventName == "beforeSubmitPrompt")
            {
                if (!string.IsNullOrEmpty(gen) && s.GenerationId == gen && s.StartedAt.HasValue && !s.StopSeen) return;
                if (!string.IsNullOrEmpty(gen) && !string.IsNullOrEmpty(s.GenerationId) && gen != s.GenerationId) AddRetired(s, s.GenerationId);
                ResetTurn(s, now);
                if (!string.IsNullOrEmpty(gen)) s.GenerationId = gen;
            }
            else
            {
                if (!string.IsNullOrEmpty(gen) && !string.IsNullOrEmpty(s.GenerationId) && gen != s.GenerationId) return;
                if (!s.StartedAt.HasValue) ResetTurn(s, now);
                if (!string.IsNullOrEmpty(gen) && string.IsNullOrEmpty(s.GenerationId)) s.GenerationId = gen;
            }

            // stop ends this generation; a late response or thought must not restart it
            if (s.StopSeen) return;

            string toolName = e.Get("tool_name");
            string toolUseId = e.Get("tool_use_id");
            switch (eventName)
            {
                case "beforeSubmitPrompt":
                {
                    s.Prompt = Text.Shorten(e.Get("prompt"), 60);
                    long hostWindow;
                    if (long.TryParse(e.Get("host_window"), NumberStyles.Integer, CultureInfo.InvariantCulture, out hostWindow) && hostWindow != 0)
                    {
                        s.HostWindow = hostWindow;
                        s.HostProcess = e.Get("host_process");
                    }
                    s.Headline = L.T("正在处理", "Working");
                    s.Activity = L.T("已收到问题", "Got your question");
                    s.Detail = s.Prompt;
                    s.Accent = "blue";
                    s.Pose = "think";
                    AddHistory(s, s.Activity);
                    break;
                }
                case "preToolUse":
                {
                    if (!string.IsNullOrEmpty(toolUseId))
                    {
                        if (s.CompletedToolIds.Contains(toolUseId) || s.OpenToolIds.Contains(toolUseId)) return;
                        s.OpenToolIds.Add(toolUseId);
                        s.OpenTools = s.OpenToolIds.Count;
                    }
                    else
                    {
                        s.OpenTools++;
                    }
                    s.AnswerSeen = false;
                    EnterRunning(s);
                    s.Steps++;
                    s.StepStartedAt = now;
                    s.Pose = ToolPose(toolName);
                    s.Activity = ToolPhrase(toolName, false);
                    s.Detail = ToolDetail(e);
                    AddHistory(s, string.IsNullOrEmpty(s.Detail) ? s.Activity : s.Activity + " " + s.Detail);
                    break;
                }
                case "postToolUse":
                {
                    bool hadOpenTool = false;
                    if (!string.IsNullOrEmpty(toolUseId))
                    {
                        if (s.CompletedToolIds.Contains(toolUseId)) return;
                        hadOpenTool = s.OpenToolIds.Remove(toolUseId);
                        s.CompletedToolIds.Add(toolUseId);
                        s.OpenTools = s.OpenToolIds.Count;
                    }
                    else if (s.OpenTools > 0)
                    {
                        hadOpenTool = true;
                        s.OpenTools--;
                    }
                    if (!hadOpenTool)
                    {
                        EnterRunning(s);
                        s.Steps++;
                    }
                    s.StepStartedAt = now;
                    if (!string.IsNullOrEmpty(toolName))
                    {
                        s.Pose = ToolPose(toolName);
                        s.Activity = ToolPhrase(toolName, true);
                    }
                    string detail = ToolDetail(e);
                    if (!string.IsNullOrEmpty(detail)) s.Detail = detail;
                    if (!hadOpenTool) AddHistory(s, string.IsNullOrEmpty(s.Detail) ? s.Activity : s.Activity + " " + s.Detail);
                    UpdateAnswerState(s, now);
                    break;
                }
                case "postToolUseFailure":
                {
                    bool hadOpenTool = false;
                    if (!string.IsNullOrEmpty(toolUseId))
                    {
                        if (s.CompletedToolIds.Contains(toolUseId)) return;
                        hadOpenTool = s.OpenToolIds.Remove(toolUseId);
                        s.CompletedToolIds.Add(toolUseId);
                        s.OpenTools = s.OpenToolIds.Count;
                    }
                    else if (s.OpenTools > 0)
                    {
                        hadOpenTool = true;
                        s.OpenTools--;
                    }
                    EnterRunning(s);
                    if (!hadOpenTool) s.Steps++;
                    s.Accent = "orange";
                    s.Pose = "error";
                    s.Activity = L.T("这一步没有成功", "This step failed");
                    s.Detail = Text.Shorten(e.Get("error_message"), 72);
                    s.StepStartedAt = now;
                    AddHistory(s, s.Activity + " " + s.Detail);
                    break;
                }
                case "afterAgentThought":
                {
                    if (s.AnswerSeen)
                    {
                        UpdateAnswerState(s, now);
                        return;
                    }
                    EnterRunning(s);
                    s.Steps++;
                    s.StepStartedAt = now;
                    s.Pose = "think";
                    s.Activity = L.T("正在思考", "Thinking");
                    s.Detail = "";
                    AddHistory(s, s.Activity);
                    break;
                }
                case "afterAgentResponse":
                {
                    s.AnswerSeen = true;
                    string snippet = Text.Shorten(e.Get("text"), 140);
                    if (!string.IsNullOrEmpty(snippet)) s.Snippet = snippet;
                    if (s.WorkOutstanding)
                    {
                        s.Activity = L.T("正在收尾", "Wrapping up");
                        if (!string.IsNullOrEmpty(s.Snippet)) s.Detail = s.Snippet;
                    }
                    UpdateAnswerState(s, now);
                    AddHistory(s, L.T("回复写好了", "Reply is ready"));
                    break;
                }
                case "subagentStart":
                {
                    s.AnswerSeen = false;
                    EnterRunning(s);
                    s.Steps++;
                    s.StepStartedAt = now;
                    s.Pose = "delegate";
                    s.Activity = L.T("正在处理子任务", "Running a subtask");
                    s.Detail = Text.Shorten(e.Get("task"), 72);
                    AddHistory(s, s.Activity + " " + s.Detail);
                    break;
                }
                case "subagentStop":
                {
                    s.StepStartedAt = now;
                    if (s.AnswerSeen && !s.WorkOutstanding)
                    {
                        UpdateAnswerState(s, now);
                    }
                    else
                    {
                        s.Phase = "running";
                        s.SettleUntil = null;
                        s.DoneUntil = null;
                        s.Headline = L.T("正在处理", "Working");
                        s.Activity = L.T("子任务已返回，继续处理", "Subtask returned, carrying on");
                        AddHistory(s, s.Activity);
                    }
                    break;
                }
                case "stop":
                {
                    string status = e.Get("status");
                    CompleteTurn(s, string.IsNullOrEmpty(status) ? "completed" : status, now);
                    break;
                }
                default:
                {
                    if (!s.AnswerSeen)
                    {
                        EnterRunning(s);
                        s.Activity = L.T("正在处理", "Working");
                        s.StepStartedAt = now;
                    }
                    break;
                }
            }
        }

        static bool IsTool(string name, params string[] names)
        {
            foreach (string candidate in names)
            {
                if (name == candidate) return true;
            }
            return false;
        }

        public static string ToolPose(string tool)
        {
            tool = tool ?? "";
            if (tool.StartsWith("MCP:", StringComparison.Ordinal)) return "web";
            if (IsTool(tool, "Read", "TabRead")) return "read";
            if (IsTool(tool, "Write", "StrReplace", "Edit", "EditNotebook", "Delete")) return "write";
            if (tool == "Shell") return "terminal";
            if (IsTool(tool, "Grep", "Glob", "SemanticSearch")) return "search";
            if (IsTool(tool, "WebSearch", "WebFetch")) return "web";
            if (tool == "Task") return "delegate";
            return "think";
        }

        public static string ToolPhrase(string tool, bool finished)
        {
            tool = tool ?? "";
            string zh;
            string en;
            if (tool.StartsWith("MCP:", StringComparison.Ordinal)) { zh = "调用外部工具"; en = "calling an external tool"; }
            else if (IsTool(tool, "Read", "TabRead")) { zh = "读取"; en = "reading"; }
            else if (IsTool(tool, "Write", "StrReplace", "Edit", "EditNotebook", "Delete")) { zh = "修改"; en = "editing"; }
            else if (tool == "Shell") { zh = "执行命令"; en = "running a command"; }
            else if (IsTool(tool, "Grep", "Glob", "SemanticSearch")) { zh = "搜索"; en = "searching"; }
            else if (tool == "Task") { zh = "派生子任务"; en = "starting a subtask"; }
            else if (IsTool(tool, "WebSearch", "WebFetch")) { zh = "查找资料"; en = "looking things up"; }
            else if (string.IsNullOrWhiteSpace(tool)) { zh = "处理"; en = "working"; }
            else { zh = "使用 " + tool; en = "using " + tool; }
            if (L.English)
            {
                string phrase = finished ? "Done " + en : en;
                return char.ToUpperInvariant(phrase[0]) + phrase.Substring(1);
            }
            return (finished ? "已完成" : "正在") + zh;
        }

        static readonly string[] DetailKeys = { "command", "file_path", "path", "target_file", "pattern", "query", "url", "glob" };

        static string PropertyText(object value)
        {
            if (value == null) return "";
            string text = value as string;
            if (text != null)
            {
                string trimmed = text.Trim();
                if (trimmed.StartsWith("{") || trimmed.StartsWith("["))
                {
                    try
                    {
                        return PropertyText(new JavaScriptSerializer().DeserializeObject(trimmed));
                    }
                    catch (ArgumentException) { return Text.Shorten(text, 80); }
                    catch (InvalidOperationException) { return Text.Shorten(text, 80); }
                }
                return Text.Shorten(text, 80);
            }
            IDictionary map = value as IDictionary;
            if (map == null) return "";
            foreach (string key in DetailKeys)
            {
                object item = map[key];
                if (item != null && !(item is IDictionary))
                {
                    string found = Convert.ToString(item, CultureInfo.InvariantCulture);
                    if (!string.IsNullOrEmpty(found)) return found;
                }
            }
            return "";
        }

        static string ToolDetail(HookEvent e)
        {
            string direct = PropertyText(e.Value("tool_input"));
            foreach (string key in new string[] { "agent_message", "file_path", "command", "task" })
            {
                if (!string.IsNullOrWhiteSpace(direct)) break;
                direct = e.Get(key);
            }
            if (direct == null) direct = "";
            if (Regex.IsMatch(direct, @"^[A-Za-z]:\\") || direct.StartsWith("/"))
            {
                string leaf = Text.Leaf(direct);
                if (!string.IsNullOrWhiteSpace(leaf)) direct = leaf;
            }
            return Text.Shorten(direct, 72);
        }

        // ---- lifecycle ----

        public void UpdateLifecycle(DateTime now)
        {
            foreach (string key in Order.ToArray())
            {
                Session s = Sessions[key];
                if (s.Phase == "settling" && s.SettleUntil.HasValue && now >= s.SettleUntil.Value)
                {
                    SoftComplete(s, now);
                    s.UpdatedAt = now;
                }
                if ((s.Phase == "running" || s.Phase == "settling") && now >= s.UpdatedAt.AddMinutes(StaleMinutes))
                {
                    ExpireStaleTurn(s, now);
                }
            }
        }

        public bool ShouldClose(DateTime now)
        {
            if (Order.Count == 0) return false;
            DateTime? latest = null;
            foreach (string key in Order)
            {
                Session s = Sessions[key];
                if (s.Phase != "done") return false;
                if (s.DoneUntil.HasValue && (!latest.HasValue || s.DoneUntil.Value > latest.Value)) latest = s.DoneUntil;
            }
            return latest.HasValue && now >= latest.Value;
        }

        public void PlayPendingSounds(DateTime now)
        {
            Session chosen = null;
            foreach (string key in Order)
            {
                Session s = Sessions[key];
                if (!s.SoundPending) continue;
                if (chosen == null || s.Accent == "red" || (s.Accent == "orange" && chosen.Accent != "red")) chosen = s;
            }
            if (chosen == null) return;
            bool quiet = Settings.Quiet && SessionFocused(chosen);
            if (quiet)
            {
                DateTime soon = now.AddSeconds(4);
                foreach (Session s in Sessions.Values)
                {
                    if (!s.SoundPending || (s.DoneUntil.HasValue && s.DoneUntil.Value <= soon)) continue;
                    s.DoneUntil = soon;
                }
            }
            if (!Settings.Mute && !quiet)
            {
                Sound(chosen.Accent == "red" ? "error" : chosen.Accent == "orange" ? "warn" : "done");
            }
            foreach (Session s in Sessions.Values)
            {
                if (!s.SoundPending) continue;
                s.SoundPending = false;
                s.SoundPlayed = true;
            }
        }

        // ---- focus and jumping back ----

        public static bool TerminalHost(Session s)
        {
            return s != null && s.HostWindow != 0 && s.HostProcess != "Cursor";
        }

        public static bool CanJump(Session s)
        {
            return TerminalHost(s) || (s != null && Text.Leaf(s.WorkspaceRoot).Length > 0);
        }

        public bool SessionFocused(Session s)
        {
            string[] info = Foreground();
            if (info == null || info.Length < 3) return false;
            if (TerminalHost(s)) return info[2] == s.HostWindow.ToString(CultureInfo.InvariantCulture);
            string leaf = s == null ? "" : Text.Leaf(s.WorkspaceRoot);
            if (leaf.Length == 0) return false;
            return info[0] == "Cursor" && info[1].IndexOf(leaf, StringComparison.OrdinalIgnoreCase) >= 0;
        }

        // Returns the session whose window should come forward, or null.
        public Session ClickSession(string key)
        {
            Session s;
            if (string.IsNullOrWhiteSpace(key) || !Sessions.TryGetValue(key, out s)) return null;
            if (s.Phase == "done")
            {
                RemoveSession(key);
                return null;
            }
            if (PinnedKey == key)
            {
                PinnedKey = "";
                return null;
            }
            PinnedKey = key;
            return s;
        }

        // ---- presentation ----

        public string CurrentPose(Session s, DateTime now)
        {
            if (s.Phase == "idle") return "idle";
            if (s.Phase == "done" || s.Phase == "settling")
            {
                if (s.Accent == "red") return "error";
                if (s.Accent == "orange") return s.Pose == "sleep" ? "sleep" : "stop";
                return "done";
            }
            if (s.Accent == "red") return "error";
            if (s.Phase == "running" && !s.WorkOutstanding && s.StepStartedAt.HasValue && (now - s.StepStartedAt.Value).TotalSeconds >= DozeSeconds)
            {
                return "sleep";
            }
            return string.IsNullOrWhiteSpace(s.Pose) ? "think" : s.Pose;
        }

        public string SpendText(Session s)
        {
            if (s.SpendStart < 0 || s.SpendEnd < 0) return "";
            long delta = s.SpendEnd - s.SpendStart;
            if (delta <= 0) return "";
            return (s.SpendShared ? L.T("期间约 ", "about ") : L.T("本轮约 ", "this turn ~")) + Text.Money(delta);
        }

        public string MetaText(Session s, DateTime now)
        {
            if (s.Phase == "done")
            {
                string spend = SpendText(s);
                return string.IsNullOrEmpty(spend) ? s.Summary : s.Summary + " · " + spend;
            }
            if (s.StartedAt.HasValue)
            {
                string text = L.T("已用 ", "") + Text.Elapsed(s.StartedAt, now) + L.T(" · 第 " + s.Steps + " 步", " · step " + s.Steps);
                return string.IsNullOrEmpty(s.Project) ? text : text + " · " + s.Project;
            }
            return L.T("等待这一轮开始", "Waiting for a turn");
        }

        public string Hint(Session s, DateTime now)
        {
            bool jump = CanJump(s);
            if (s.Phase == "done" && s.DoneUntil.HasValue)
            {
                int left = Math.Max(1, (int)Math.Ceiling((s.DoneUntil.Value - now).TotalSeconds));
                return jump
                    ? L.T("点标题跳回对话 · 点别处关掉 · ", "Title: back to chat · elsewhere: close · ") + left + "s"
                    : L.T("点一下关掉 · ", "Click to close · ") + left + "s";
            }
            if (s.Phase == "settling") return L.T("回复已经写好了", "The reply is ready");
            if (s.Phase == "running")
            {
                return jump ? L.T("点标题跳回对话 · 点别处看步骤", "Title: back to chat · elsewhere: steps") : L.T("点一下可以看步骤", "Click to see the steps");
            }
            return L.T("等你发来问题", "Waiting for your question");
        }

        string SessionLabel(Session s, string key)
        {
            if (!string.IsNullOrEmpty(s.Prompt)) return Text.Shorten(s.Prompt, 16);
            if (!string.IsNullOrEmpty(s.Project)) return Text.Shorten(s.Project, 16);
            return key.Length > 6 ? key.Substring(0, 6) : key;
        }

        string SessionLine(Session s, string key)
        {
            string mark = "run";
            string state = L.T("进行中", "running");
            if (s.Phase == "done" && s.Accent == "red") { mark = "bad"; state = L.T("出错", "failed"); }
            else if (s.Phase == "done" && s.Accent == "orange") { mark = "warn"; state = L.T("已中断", "stopped"); }
            else if (s.Phase == "done") { mark = "done"; state = L.T("已完成", "done"); }
            else if (s.Phase == "settling") { mark = "wait"; state = L.T("收尾", "wrapping up"); }
            else if (s.Accent == "orange" || s.Accent == "red") { mark = "warn"; state = L.T("这一步失败", "step failed"); }
            else if (!string.IsNullOrEmpty(s.Activity)) state = s.Activity;
            string extra = s.Phase == "running" && !string.IsNullOrEmpty(s.Detail) ? " · " + Text.Shorten(s.Detail, 18) : "";
            string prefix = PinnedKey == key ? L.T("当前 · ", "pinned · ") : "";
            return mark + "|" + key + "|" + prefix + SessionLabel(s, key) + " · " + state + extra;
        }

        static int Rank(Session s)
        {
            if (s.Phase == "done" && s.Accent == "red") return 0;
            if (s.Phase == "running" || s.Phase == "settling") return 1;
            if (s.Phase == "done" && s.Accent == "orange") return 2;
            return 3;
        }

        public string BoardText()
        {
            List<Session> rows = new List<Session>();
            foreach (string key in Order) rows.Add(Sessions[key]);
            rows.Sort(delegate(Session a, Session b)
            {
                int byRank = Rank(a).CompareTo(Rank(b));
                return byRank != 0 ? byRank : b.UpdatedAt.CompareTo(a.UpdatedAt);
            });
            List<string> lines = new List<string>();
            foreach (Session s in rows)
            {
                lines.Add(SessionLine(s, s.Key));
                if (lines.Count >= 8) break;
            }
            return string.Join("\n", lines.ToArray());
        }

        public View Presentation(DateTime now)
        {
            View view = new View();
            if (Order.Count <= 1)
            {
                Session s = Order.Count == 1 ? Sessions[Order[0]] : _idle;
                if (Order.Count == 1) Current = s;
                view.Phase = s.Phase;
                view.Accent = s.Accent;
                view.Headline = s.Headline;
                view.Activity = s.Activity;
                view.Detail = s.Detail;
                view.Meta = MetaText(s, now);
                view.Hint = Hint(s, now);
                view.Pose = CurrentPose(s, now);
                view.History = string.Join("\n", s.History.ToArray());
                return view;
            }

            int running = 0, bad = 0, aborted = 0, finished = 0;
            Session newestRun = null;
            foreach (string key in Order)
            {
                Session s = Sessions[key];
                if (s.Phase == "done" && s.Accent == "red") bad++;
                else if (s.Phase == "done" && s.Accent == "orange") aborted++;
                else if (s.Phase == "done") finished++;
                else
                {
                    running++;
                    if (newestRun == null || s.UpdatedAt >= newestRun.UpdatedAt) newestRun = s;
                }
            }
            List<string> parts = new List<string>();
            if (running > 0) parts.Add(L.T(running + " 个进行中", running + " running"));
            if (bad > 0) parts.Add(L.T(bad + " 个出错", bad + " failed"));
            if (aborted > 0) parts.Add(L.T(aborted + " 个已中断", aborted + " stopped"));
            if (finished > 0) parts.Add(L.T(finished + " 个已完成", finished + " done"));
            view.Headline = string.Join(" · ", parts.ToArray());
            view.Accent = "green";
            view.Pose = "done";
            view.Phase = running > 0 ? "running" : "done";
            if (bad > 0)
            {
                view.Accent = "red";
                view.Pose = "error";
            }
            else if (running > 0)
            {
                view.Accent = "blue";
                view.Pose = newestRun != null && !string.IsNullOrEmpty(newestRun.Pose) ? newestRun.Pose : "think";
            }
            else if (aborted > 0)
            {
                view.Accent = "orange";
                view.Pose = "stop";
            }
            view.Hint = L.T("点一行只处理这一路", "Click a row to follow it");

            Session focus = null;
            if (!string.IsNullOrEmpty(PinnedKey) && Sessions.TryGetValue(PinnedKey, out focus) && focus.Phase == "done")
            {
                PinnedKey = "";
                focus = null;
            }
            if (focus != null)
            {
                view.Headline = SessionLabel(focus, focus.Key);
                view.Activity = focus.Activity;
                view.Detail = focus.Detail;
                view.Accent = string.IsNullOrEmpty(focus.Accent) ? "blue" : focus.Accent;
                view.Pose = string.IsNullOrEmpty(focus.Pose) ? "think" : focus.Pose;
                view.Phase = "running";
                view.Hint = L.T("再点这一行回到总览", "Click the row again for the overview");
            }
            if (focus == null && view.Phase == "done")
            {
                DateTime? latest = null;
                foreach (Session s in Sessions.Values)
                {
                    if (s.DoneUntil.HasValue && (!latest.HasValue || s.DoneUntil.Value > latest.Value)) latest = s.DoneUntil;
                }
                int left = latest.HasValue ? Math.Max(1, (int)Math.Ceiling((latest.Value - now).TotalSeconds)) : 1;
                view.Hint = L.T("点一下关掉 · ", "Click to close · ") + left + "s";
            }
            view.Meta = L.T("共 " + Order.Count + " 个会话", Order.Count + " sessions");
            view.History = BoardText();
            return view;
        }

        // ---- quota: turn spend and alerts ----

        public void UpdateTurnSpend(QuotaSnapshot snapshot)
        {
            if (snapshot == null || !snapshot.Ok || snapshot.TotalSpendCents < 0) return;
            foreach (Session s in Sessions.Values)
            {
                if (!s.StartedAt.HasValue) continue;
                if (s.SpendStart < 0 && s.Phase != "done" && snapshot.FetchedAt >= s.StartedAt.Value.AddSeconds(-30))
                {
                    s.SpendStart = snapshot.TotalSpendCents;
                }
                if (s.Phase == "done" && s.DoneAt.HasValue && s.SpendStart >= 0 && snapshot.FetchedAt >= s.DoneAt.Value)
                {
                    s.SpendEnd = snapshot.TotalSpendCents;
                }
            }
        }

        public bool NeedsSpendSample()
        {
            foreach (Session s in Sessions.Values)
            {
                if (s.Phase != "done" && s.StartedAt.HasValue && s.SpendStart < 0) return true;
            }
            return false;
        }

        // Cursor books the last requests a few seconds late, so keep sampling briefly after a turn ends.
        public bool NeedsSpendFollowUp(QuotaSnapshot snapshot, DateTime now)
        {
            foreach (Session s in Sessions.Values)
            {
                if (s.Phase != "done" || !s.DoneAt.HasValue || s.SpendStart < 0) continue;
                if (now >= s.DoneAt.Value.AddSeconds(15)) continue;
                if (snapshot == null || snapshot.FetchedAt < s.DoneAt.Value.AddSeconds(5)) return true;
            }
            return false;
        }

        public void UpdateQuotaAlert(QuotaSnapshot snapshot, DateTime now)
        {
            if (snapshot == null || !snapshot.Ok) return;
            double total = snapshot.HeadlinePercent;
            double api = snapshot.ApiPercent;
            bool apiWins = api > total;
            double percent = apiWins ? api : total;
            int level = percent >= 100 ? 100 : percent >= 80 ? 80 : 0;
            if (level == 0) return;
            string cycle = snapshot.CycleEnd == DateTime.MinValue ? "unknown" : snapshot.CycleEnd.ToUniversalTime().ToString("yyyyMMddHH", CultureInfo.InvariantCulture);
            int alerted = 0;
            try
            {
                if (!string.IsNullOrEmpty(QuotaAlertPath) && File.Exists(QuotaAlertPath))
                {
                    string[] saved = File.ReadAllText(QuotaAlertPath, Encoding.UTF8).Trim().Split('|');
                    if (saved.Length == 2 && saved[0] == cycle) int.TryParse(saved[1], out alerted);
                }
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            if (level <= alerted) return;
            try
            {
                if (!string.IsNullOrEmpty(QuotaAlertPath))
                {
                    Directory.CreateDirectory(Path.GetDirectoryName(QuotaAlertPath));
                    File.WriteAllText(QuotaAlertPath, cycle + "|" + level, new UTF8Encoding(false));
                }
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            string name = apiWins ? L.T("本期 API 额度", "API usage") : L.T("本期额度", "Usage");
            QuotaNotice = level >= 100
                ? name + L.T("已用完", " is used up")
                : name + L.T("已用 ", " at ") + (int)Math.Floor(percent) + "%";
            QuotaNoticeUntil = now.AddSeconds(15);
            if (!Settings.Mute) Sound("alert");
        }

        public string ActiveQuotaNotice(DateTime now)
        {
            return !string.IsNullOrEmpty(QuotaNotice) && QuotaNoticeUntil.HasValue && now < QuotaNoticeUntil.Value ? QuotaNotice : "";
        }
    }
}
