using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Net;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;

namespace CursorDoneBuddy
{
    public sealed class QuotaSnapshot
    {
        public bool Ok;
        public string Error = "";
        public string PlanName = "";
        public double AutoPercent = -1;
        public double ApiPercent = -1;
        public double TotalPercent = -1;
        public long TotalSpendCents = -1;
        public long IncludedSpendCents = -1;
        public long BonusSpendCents = -1;
        public long LimitCents = -1;
        public long OnDemandUsedCents = -1;
        public long OnDemandLimitCents = -1;
        public DateTime CycleStart = DateTime.MinValue;
        public DateTime CycleEnd = DateTime.MinValue;
        public DateTime FetchedAt = DateTime.MinValue;

        public static QuotaSnapshot Failure(string error, DateTime now)
        {
            QuotaSnapshot snapshot = new QuotaSnapshot();
            snapshot.Error = error ?? "";
            snapshot.FetchedAt = now;
            return snapshot;
        }

        public QuotaSnapshot Copy()
        {
            return (QuotaSnapshot)MemberwiseClone();
        }

        public double HeadlinePercent
        {
            get
            {
                if (TotalPercent >= 0) return TotalPercent;
                if (TotalSpendCents >= 0 && LimitCents > 0) return TotalSpendCents * 100.0 / LimitCents;
                return -1;
            }
        }

        public int DaysLeft(DateTime now)
        {
            if (CycleEnd == DateTime.MinValue) return -1;
            double days = (CycleEnd - now).TotalDays;
            if (days < 0) return 0;
            return (int)Math.Ceiling(days);
        }

        public static string Money(long cents)
        {
            if (cents < 0) return "";
            return "$" + (cents / 100.0).ToString("0.00", CultureInfo.InvariantCulture);
        }

        public static string Percent(double value)
        {
            if (value < 0) return "--";
            if (value > 0 && value < 1) return "<1%";
            return Math.Round(value).ToString(CultureInfo.InvariantCulture) + "%";
        }

        public string Serialize()
        {
            StringBuilder sb = new StringBuilder();
            Line(sb, "ok", Ok ? "1" : "0");
            Line(sb, "error", Error);
            Line(sb, "plan", PlanName);
            Line(sb, "auto", AutoPercent.ToString("R", CultureInfo.InvariantCulture));
            Line(sb, "api", ApiPercent.ToString("R", CultureInfo.InvariantCulture));
            Line(sb, "total", TotalPercent.ToString("R", CultureInfo.InvariantCulture));
            Line(sb, "spend", TotalSpendCents.ToString(CultureInfo.InvariantCulture));
            Line(sb, "included", IncludedSpendCents.ToString(CultureInfo.InvariantCulture));
            Line(sb, "bonus", BonusSpendCents.ToString(CultureInfo.InvariantCulture));
            Line(sb, "limit", LimitCents.ToString(CultureInfo.InvariantCulture));
            Line(sb, "ondemand", OnDemandUsedCents.ToString(CultureInfo.InvariantCulture));
            Line(sb, "ondemandLimit", OnDemandLimitCents.ToString(CultureInfo.InvariantCulture));
            Line(sb, "start", CycleStart.ToUniversalTime().Ticks.ToString(CultureInfo.InvariantCulture));
            Line(sb, "end", CycleEnd.ToUniversalTime().Ticks.ToString(CultureInfo.InvariantCulture));
            Line(sb, "fetched", FetchedAt.ToUniversalTime().Ticks.ToString(CultureInfo.InvariantCulture));
            return sb.ToString();
        }

        public static QuotaSnapshot Deserialize(string text)
        {
            if (string.IsNullOrEmpty(text)) return null;
            Dictionary<string, string> map = new Dictionary<string, string>();
            foreach (string raw in text.Split('\n'))
            {
                int eq = raw.IndexOf('=');
                if (eq <= 0) continue;
                map[raw.Substring(0, eq)] = raw.Substring(eq + 1).TrimEnd('\r');
            }
            QuotaSnapshot s = new QuotaSnapshot();
            s.Ok = Get(map, "ok") == "1";
            s.Error = Get(map, "error");
            s.PlanName = Get(map, "plan");
            s.AutoPercent = Num(map, "auto");
            s.ApiPercent = Num(map, "api");
            s.TotalPercent = Num(map, "total");
            s.TotalSpendCents = Long(map, "spend");
            s.IncludedSpendCents = Long(map, "included");
            s.BonusSpendCents = Long(map, "bonus");
            s.LimitCents = Long(map, "limit");
            s.OnDemandUsedCents = Long(map, "ondemand");
            s.OnDemandLimitCents = Long(map, "ondemandLimit");
            s.CycleStart = Time(map, "start");
            s.CycleEnd = Time(map, "end");
            s.FetchedAt = Time(map, "fetched");
            if (s.FetchedAt == DateTime.MinValue) return null;
            return s;
        }

        static void Line(StringBuilder sb, string key, string value)
        {
            sb.Append(key).Append('=').Append((value ?? "").Replace('\n', ' ').Replace('\r', ' ')).Append('\n');
        }

        static string Get(Dictionary<string, string> map, string key)
        {
            string value;
            return map.TryGetValue(key, out value) ? value : "";
        }

        static double Num(Dictionary<string, string> map, string key)
        {
            double value;
            return double.TryParse(Get(map, key), NumberStyles.Float, CultureInfo.InvariantCulture, out value) ? value : -1;
        }

        static long Long(Dictionary<string, string> map, string key)
        {
            long value;
            return long.TryParse(Get(map, key), NumberStyles.Integer, CultureInfo.InvariantCulture, out value) ? value : -1;
        }

        static DateTime Time(Dictionary<string, string> map, string key)
        {
            long ticks = Long(map, key);
            if (ticks <= 0 || ticks >= DateTime.MaxValue.Ticks) return DateTime.MinValue;
            return new DateTime(ticks, DateTimeKind.Utc).ToLocalTime();
        }
    }

    public static class QuotaParser
    {
        public static QuotaSnapshot Parse(string usageJson, string planJson, string membership, DateTime now)
        {
            JavaScriptSerializer json = new JavaScriptSerializer();
            IDictionary usage = json.DeserializeObject(usageJson ?? "") as IDictionary;
            if (usage == null) return QuotaSnapshot.Failure(L.T("用量接口返回了无法识别的内容", "The usage API returned something unexpected"), now);

            QuotaSnapshot s = new QuotaSnapshot();
            s.Ok = true;
            s.FetchedAt = now;
            s.CycleStart = EpochMs(usage["billingCycleStart"]);
            s.CycleEnd = EpochMs(usage["billingCycleEnd"]);

            IDictionary plan = usage["planUsage"] as IDictionary;
            if (plan != null)
            {
                s.AutoPercent = Number(plan["autoPercentUsed"], -1);
                s.ApiPercent = Number(plan["apiPercentUsed"], -1);
                s.TotalPercent = Number(plan["totalPercentUsed"], -1);
                s.TotalSpendCents = (long)Number(plan["totalSpend"], -1);
                s.IncludedSpendCents = (long)Number(plan["includedSpend"], -1);
                s.BonusSpendCents = (long)Number(plan["bonusSpend"], -1);
                s.LimitCents = (long)Number(plan["limit"], -1);
            }

            IDictionary spend = usage["spendLimitUsage"] as IDictionary;
            if (spend != null)
            {
                double limit = Number(spend["individualLimit"], -1);
                double used = Number(spend["individualUsed"], -1);
                if (limit < 0)
                {
                    limit = Number(spend["pooledLimit"], -1);
                    used = Number(spend["pooledUsed"], -1);
                }
                if (limit > 0)
                {
                    s.OnDemandLimitCents = (long)limit;
                    s.OnDemandUsedCents = used < 0 ? 0 : (long)used;
                }
            }

            IDictionary info = null;
            if (!string.IsNullOrEmpty(planJson))
            {
                try
                {
                    IDictionary root = json.DeserializeObject(planJson) as IDictionary;
                    if (root != null) info = root["planInfo"] as IDictionary;
                }
                catch (ArgumentException) { }
                catch (InvalidOperationException) { }
            }
            if (info != null) s.PlanName = Text(info["planName"]);
            if (string.IsNullOrEmpty(s.PlanName)) s.PlanName = MembershipLabel(membership);
            return s;
        }

        public static string MembershipLabel(string membership)
        {
            if (string.IsNullOrEmpty(membership)) return "";
            switch (membership.Trim().ToLowerInvariant())
            {
                case "free": return "Free";
                case "free_trial": return L.T("Pro 试用", "Pro trial");
                case "pro": return "Pro";
                case "pro_plus": return "Pro+";
                case "ultra": return "Ultra";
                case "business": return "Teams";
                case "enterprise": return "Enterprise";
                default: return membership;
            }
        }

        static double Number(object value, double fallback)
        {
            if (value == null) return fallback;
            if (value is int) return (int)value;
            if (value is long) return (long)value;
            if (value is decimal) return (double)(decimal)value;
            if (value is double) return (double)value;
            double parsed;
            if (double.TryParse(Convert.ToString(value, CultureInfo.InvariantCulture), NumberStyles.Float, CultureInfo.InvariantCulture, out parsed)) return parsed;
            return fallback;
        }

        static string Text(object value)
        {
            return value == null ? "" : Convert.ToString(value, CultureInfo.InvariantCulture);
        }

        static DateTime EpochMs(object value)
        {
            double ms = Number(value, -1);
            if (ms <= 0) return DateTime.MinValue;
            try
            {
                return new DateTime(1970, 1, 1, 0, 0, 0, DateTimeKind.Utc).AddMilliseconds(ms).ToLocalTime();
            }
            catch (ArgumentOutOfRangeException)
            {
                return DateTime.MinValue;
            }
        }
    }

    public sealed class QuotaService : IDisposable
    {
        const string UsageUrl = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";
        const string PlanUrl = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetPlanInfo";
        static readonly TimeSpan MinGap = TimeSpan.FromSeconds(20);
        static readonly TimeSpan UrgentGap = TimeSpan.FromSeconds(3);
        static readonly TimeSpan Interval = TimeSpan.FromMinutes(5);
        static readonly TimeSpan PlanInterval = TimeSpan.FromHours(6);

        readonly AutoResetEvent _wake = new AutoResetEvent(false);
        volatile QuotaSnapshot _latest;
        volatile bool _stopping;
        volatile bool _urgent;
        int _version;
        Thread _worker;
        string _planJson;
        DateTime _planFetchedAt = DateTime.MinValue;

        public QuotaService()
        {
            string appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
            DatabasePath = Path.Combine(appData, @"Cursor\User\globalStorage\state.vscdb");
        }

        public string DatabasePath { get; set; }
        public string CachePath { get; set; }

        public QuotaSnapshot Latest
        {
            get { return _latest; }
        }

        public int Version
        {
            get { return Thread.VolatileRead(ref _version); }
        }

        public void Publish(QuotaSnapshot snapshot)
        {
            _latest = snapshot;
            Interlocked.Increment(ref _version);
        }

        public void Start()
        {
            if (_worker != null) return;
            LoadCache();
            _worker = new Thread(Run);
            _worker.IsBackground = true;
            _worker.Name = "CursorQuota";
            _worker.Start();
        }

        public void RequestRefresh()
        {
            RequestRefresh(false);
        }

        public void RequestRefresh(bool urgent)
        {
            if (urgent) _urgent = true;
            _wake.Set();
        }

        public void Dispose()
        {
            _stopping = true;
            _wake.Set();
        }

        void Run()
        {
            while (!_stopping)
            {
                QuotaSnapshot current = _latest;
                DateTime last = current == null ? DateTime.MinValue : current.FetchedAt;
                TimeSpan since = DateTime.Now - last;
                if (since < TimeSpan.Zero) since = TimeSpan.Zero;
                TimeSpan gap = _urgent ? UrgentGap : MinGap;
                if (since < gap)
                {
                    _wake.WaitOne(gap - since);
                    continue;
                }
                _urgent = false;
                QuotaSnapshot next = FetchOnce();
                if (_stopping) return;
                if (!next.Ok && current != null && current.Ok)
                {
                    QuotaSnapshot stale = current.Copy();
                    stale.Error = next.Error;
                    stale.FetchedAt = next.FetchedAt;
                    next = stale;
                }
                else if (next.Ok)
                {
                    next.Error = "";
                }
                Publish(next);
                SaveCache(next);
                _wake.WaitOne(Interval);
            }
        }

        public QuotaSnapshot FetchOnce()
        {
            DateTime now = DateTime.Now;
            string token;
            string membership;
            try
            {
                if (string.IsNullOrEmpty(DatabasePath) || !File.Exists(DatabasePath))
                {
                    return QuotaSnapshot.Failure(L.T("没找到 Cursor 的登录数据", "No Cursor sign-in data found"), now);
                }
                token = StateDb.Read(DatabasePath, "cursorAuth/accessToken");
                membership = StateDb.Read(DatabasePath, "cursorAuth/stripeMembershipType");
            }
            catch (Exception ex)
            {
                return QuotaSnapshot.Failure(L.T("读不到 Cursor 登录数据：", "Cannot read Cursor sign-in data: ") + ex.Message, now);
            }
            if (string.IsNullOrEmpty(token)) return QuotaSnapshot.Failure(L.T("Cursor 还没有登录", "Cursor is not signed in"), now);

            try
            {
                string usage = Post(UsageUrl, token);
                if (_planJson == null || now - _planFetchedAt > PlanInterval)
                {
                    try
                    {
                        _planJson = Post(PlanUrl, token);
                        _planFetchedAt = now;
                    }
                    catch (WebException) { }
                }
                return QuotaParser.Parse(usage, _planJson, membership, now);
            }
            catch (WebException ex)
            {
                HttpWebResponse response = ex.Response as HttpWebResponse;
                if (response != null && (response.StatusCode == HttpStatusCode.Unauthorized || response.StatusCode == HttpStatusCode.Forbidden))
                {
                    return QuotaSnapshot.Failure(L.T("登录已过期，打开 Cursor 会自动续上", "Sign-in expired; opening Cursor renews it"), now);
                }
                return QuotaSnapshot.Failure(L.T("网络不通：", "Network error: ") + ex.Message, now);
            }
            catch (Exception ex)
            {
                return QuotaSnapshot.Failure(L.T("额度解析失败：", "Could not read usage: ") + ex.Message, now);
            }
        }

        static string Post(string url, string token)
        {
            try
            {
                ServicePointManager.SecurityProtocol |= (SecurityProtocolType)3072;
            }
            catch (NotSupportedException) { }
            HttpWebRequest request = (HttpWebRequest)WebRequest.Create(url);
            request.Method = "POST";
            request.Timeout = 12000;
            request.ReadWriteTimeout = 12000;
            request.ContentType = "application/json";
            request.Accept = "application/json";
            request.UserAgent = "cursor-done-notify";
            request.Headers["Authorization"] = "Bearer " + token;
            request.Headers["Connect-Protocol-Version"] = "1";
            byte[] body = Encoding.UTF8.GetBytes("{}");
            request.ContentLength = body.Length;
            using (Stream stream = request.GetRequestStream())
            {
                stream.Write(body, 0, body.Length);
            }
            using (WebResponse response = request.GetResponse())
            using (StreamReader reader = new StreamReader(response.GetResponseStream(), Encoding.UTF8))
            {
                return reader.ReadToEnd();
            }
        }

        void LoadCache()
        {
            if (string.IsNullOrEmpty(CachePath) || !File.Exists(CachePath)) return;
            try
            {
                QuotaSnapshot cached = QuotaSnapshot.Deserialize(File.ReadAllText(CachePath, Encoding.UTF8));
                if (cached != null && cached.Ok) Publish(cached);
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }

        void SaveCache(QuotaSnapshot snapshot)
        {
            if (string.IsNullOrEmpty(CachePath) || snapshot == null || !snapshot.Ok) return;
            try
            {
                string temp = CachePath + ".tmp";
                File.WriteAllText(temp, snapshot.Serialize(), new UTF8Encoding(false));
                if (File.Exists(CachePath)) File.Delete(CachePath);
                File.Move(temp, CachePath);
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
    }

    static class StateDb
    {
        const int SqliteOk = 0;
        const int SqliteRow = 100;
        const int OpenReadOnly = 0x00000001;
        static readonly IntPtr Transient = new IntPtr(-1);

        public static string Read(string path, string key)
        {
            IntPtr db;
            if (sqlite3_open_v2(Utf8z(path), out db, OpenReadOnly, IntPtr.Zero) != SqliteOk)
            {
                if (db != IntPtr.Zero) sqlite3_close(db);
                throw new IOException(L.T("无法打开 state.vscdb", "cannot open state.vscdb"));
            }
            try
            {
                sqlite3_busy_timeout(db, 2000);
                IntPtr statement;
                if (sqlite3_prepare_v2(db, Utf8z("SELECT value FROM ItemTable WHERE key = ?1"), -1, out statement, IntPtr.Zero) != SqliteOk)
                {
                    throw new IOException(L.T("state.vscdb 结构不认识", "unrecognized state.vscdb layout"));
                }
                try
                {
                    byte[] keyBytes = Encoding.UTF8.GetBytes(key);
                    sqlite3_bind_text(statement, 1, keyBytes, keyBytes.Length, Transient);
                    if (sqlite3_step(statement) != SqliteRow) return null;
                    int length = sqlite3_column_bytes(statement, 0);
                    if (length <= 0) return "";
                    byte[] buffer = new byte[length];
                    Marshal.Copy(sqlite3_column_blob(statement, 0), buffer, 0, length);
                    return Encoding.UTF8.GetString(buffer).Trim().Trim('"');
                }
                finally
                {
                    sqlite3_finalize(statement);
                }
            }
            finally
            {
                sqlite3_close(db);
            }
        }

        static byte[] Utf8z(string text)
        {
            return Encoding.UTF8.GetBytes(text + "\0");
        }

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern int sqlite3_open_v2(byte[] filename, out IntPtr db, int flags, IntPtr vfs);

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern int sqlite3_busy_timeout(IntPtr db, int ms);

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern int sqlite3_prepare_v2(IntPtr db, byte[] sql, int bytes, out IntPtr statement, IntPtr tail);

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern int sqlite3_bind_text(IntPtr statement, int index, byte[] value, int bytes, IntPtr destructor);

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern int sqlite3_step(IntPtr statement);

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern IntPtr sqlite3_column_blob(IntPtr statement, int column);

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern int sqlite3_column_bytes(IntPtr statement, int column);

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern int sqlite3_finalize(IntPtr statement);

        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.StdCall)]
        static extern int sqlite3_close(IntPtr db);
    }
}
