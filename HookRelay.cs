using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;

internal static class HookRelay
{
    private const int MaxStoredBytes = 2 * 1024 * 1024;

    private static int Main(string[] args)
    {
        string reply = args.Length > 0 ? args[0] : "empty";
        try
        {
            string body = ReadStdin();
            if (!string.IsNullOrEmpty(body))
            {
                WriteInbox(CompactInbox(body));
                EnsureHost();
            }
        }
        catch
        {
        }

        if (reply == "allow")
        {
            Console.WriteLine("{\"permission\":\"allow\"}");
        }
        else if (reply == "continue")
        {
            Console.WriteLine("{\"continue\":true}");
        }
        else
        {
            Console.WriteLine("{}");
        }

        return 0;
    }

    private static string ReadStdin()
    {
        Stream stdin = Console.OpenStandardInput();
        byte[] buffer = new byte[8192];
        MemoryStream kept = new MemoryStream();
        int stored = 0;
        int read;
        while ((read = stdin.Read(buffer, 0, buffer.Length)) > 0)
        {
            int take = Math.Min(read, MaxStoredBytes - stored);
            if (take > 0)
            {
                kept.Write(buffer, 0, take);
                stored += take;
            }
        }

        byte[] bytes = kept.ToArray();
        if (bytes.Length == 0)
        {
            return string.Empty;
        }

        if (bytes.Length >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE)
        {
            return Encoding.Unicode.GetString(bytes);
        }

        int zeros = 0;
        int sample = Math.Min(bytes.Length, 160);
        for (int i = 0; i < sample; i++)
        {
            if (bytes[i] == 0)
            {
                zeros++;
            }
        }

        if (zeros > 8)
        {
            return Encoding.Unicode.GetString(bytes);
        }

        try
        {
            return new UTF8Encoding(false, true).GetString(bytes);
        }
        catch (DecoderFallbackException)
        {
            return Encoding.Default.GetString(bytes);
        }
    }

    private static string CompactInbox(string body)
    {
        if (string.IsNullOrEmpty(body) || body.Length <= 32768)
        {
            return body;
        }

        string eventName = ExtractString(body, "hook_event_name");
        if (string.IsNullOrEmpty(eventName))
        {
            return body.Length > 48000 ? body.Substring(0, 48000) : body;
        }

        StringBuilder sb = new StringBuilder();
        sb.Append('{');
        bool first = true;
        AppendString(sb, ref first, "hook_event_name", eventName);
        AppendString(sb, ref first, "conversation_id", ExtractString(body, "conversation_id"));
        AppendString(sb, ref first, "generation_id", ExtractString(body, "generation_id"));
        AppendString(sb, ref first, "status", ExtractString(body, "status"));
        AppendString(sb, ref first, "tool_name", ExtractString(body, "tool_name"));
        AppendString(sb, ref first, "tool_use_id", ExtractString(body, "tool_use_id"));
        AppendString(sb, ref first, "subagent_id", ExtractString(body, "subagent_id"));
        AppendString(sb, ref first, "tool_call_id", ExtractString(body, "tool_call_id"));
        AppendString(sb, ref first, "error_message", Trunc(ExtractString(body, "error_message"), 180));
        AppendString(sb, ref first, "command", Trunc(ExtractString(body, "command"), 180));
        AppendString(sb, ref first, "file_path", Trunc(ExtractString(body, "file_path"), 180));
        AppendString(sb, ref first, "path", Trunc(ExtractString(body, "path"), 180));
        AppendString(sb, ref first, "target_file", Trunc(ExtractString(body, "target_file"), 180));
        AppendString(sb, ref first, "pattern", Trunc(ExtractString(body, "pattern"), 120));
        AppendString(sb, ref first, "query", Trunc(ExtractString(body, "query"), 120));
        AppendString(sb, ref first, "url", Trunc(ExtractString(body, "url"), 180));
        AppendString(sb, ref first, "glob", Trunc(ExtractString(body, "glob"), 120));
        AppendString(sb, ref first, "prompt", Trunc(ExtractString(body, "prompt"), 80));
        AppendString(sb, ref first, "task", Trunc(ExtractString(body, "task"), 120));
        AppendString(sb, ref first, "agent_message", Trunc(ExtractString(body, "agent_message"), 180));
        AppendString(sb, ref first, "text", Trunc(ExtractString(body, "text"), 180));

        Match roots = Regex.Match(body, "\"workspace_roots\"\\s*:\\s*(\\[[^\\]]*\\])");
        if (roots.Success)
        {
            if (!first) sb.Append(',');
            sb.Append("\"workspace_roots\":");
            sb.Append(roots.Groups[1].Value);
        }

        sb.Append('}');
        return sb.ToString();
    }

    private static void AppendString(StringBuilder sb, ref bool first, string name, string value)
    {
        if (string.IsNullOrEmpty(value)) return;
        if (!first) sb.Append(',');
        first = false;
        sb.Append('"').Append(EscapeJson(name)).Append("\":\"").Append(EscapeJson(value)).Append('"');
    }

    private static string Trunc(string value, int max)
    {
        if (string.IsNullOrEmpty(value) || value.Length <= max) return value;
        return value.Substring(0, max);
    }

    private static string ExtractString(string json, string name)
    {
        Match match = Regex.Match(
            json,
            "\"" + Regex.Escape(name) + "\"\\s*:\\s*\"((?:\\\\.|[^\"\\\\])*)\"");
        if (!match.Success) return null;
        return UnescapeJson(match.Groups[1].Value);
    }

    private static string UnescapeJson(string value)
    {
        if (string.IsNullOrEmpty(value) || value.IndexOf('\\') < 0) return value;
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < value.Length; i++)
        {
            char ch = value[i];
            if (ch != '\\' || i + 1 >= value.Length)
            {
                sb.Append(ch);
                continue;
            }
            char next = value[++i];
            switch (next)
            {
                case '"':
                case '\\':
                case '/':
                    sb.Append(next);
                    break;
                case 'b': sb.Append('\b'); break;
                case 'f': sb.Append('\f'); break;
                case 'n': sb.Append('\n'); break;
                case 'r': sb.Append('\r'); break;
                case 't': sb.Append('\t'); break;
                case 'u':
                    if (i + 4 < value.Length)
                    {
                        int code;
                        string hex = value.Substring(i + 1, 4);
                        if (int.TryParse(hex, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out code))
                        {
                            sb.Append((char)code);
                            i += 4;
                            break;
                        }
                    }
                    sb.Append(next);
                    break;
                default:
                    sb.Append(next);
                    break;
            }
        }
        return sb.ToString();
    }

    private static string EscapeJson(string value)
    {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < value.Length; i++)
        {
            char ch = value[i];
            switch (ch)
            {
                case '\\': sb.Append("\\\\"); break;
                case '"': sb.Append("\\\""); break;
                case '\b': sb.Append("\\b"); break;
                case '\f': sb.Append("\\f"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                case '\t': sb.Append("\\t"); break;
                default:
                    if (ch < 32) continue;
                    sb.Append(ch);
                    break;
            }
        }
        return sb.ToString();
    }

    private static void WriteInbox(string body)
    {
        string dir = Path.Combine(Path.GetTempPath(), "cursor-done-notify", "inbox");
        Directory.CreateDirectory(dir);
        string name = DateTime.UtcNow.Ticks.ToString("D20") + "-" + Guid.NewGuid().ToString("n") + ".json";
        string finalPath = Path.Combine(dir, name);
        string tempPath = finalPath + ".tmp";
        File.WriteAllText(tempPath, body, new UTF8Encoding(false));
        File.Move(tempPath, finalPath);
    }

    private static void EnsureHost()
    {
        string pidFile = Path.Combine(Path.GetTempPath(), "cursor-done-notify", "host.pid");
        using (Mutex launch = new Mutex(false, @"Local\CursorDoneNotifyLaunch"))
        {
            bool owned = false;
            try
            {
                try
                {
                    owned = launch.WaitOne(2000);
                }
                catch (AbandonedMutexException)
                {
                    owned = true;
                }
                if (!owned || HostIsRunning(pidFile))
                {
                    return;
                }

                string root = AppDomain.CurrentDomain.BaseDirectory;
                string script = Path.Combine(root, "progress-host.ps1");
                string powershell = Path.Combine(
                    Environment.SystemDirectory,
                    @"WindowsPowerShell\v1.0\powershell.exe");
                if (!File.Exists(powershell))
                {
                    powershell = "powershell.exe";
                }

                ProcessStartInfo start = new ProcessStartInfo();
                start.FileName = powershell;
                start.Arguments = "-NoProfile -STA -ExecutionPolicy Bypass -WindowStyle Hidden -File \"" + script + "\"";
                start.WorkingDirectory = root;
                start.UseShellExecute = false;
                start.CreateNoWindow = true;
                Process.Start(start);

                for (int i = 0; i < 20 && !HostIsRunning(pidFile); i++)
                {
                    Thread.Sleep(25);
                }
            }
            finally
            {
                if (owned)
                {
                    launch.ReleaseMutex();
                }
            }
        }
    }

    private static bool HostIsRunning(string pidFile)
    {
        try
        {
            if (!File.Exists(pidFile))
            {
                return false;
            }

            string text = File.ReadAllText(pidFile).Trim();
            string[] parts = text.Split('|');
            int pid;
            if (parts.Length < 2 || !int.TryParse(parts[0], out pid))
            {
                return false;
            }

            Process process = Process.GetProcessById(pid);
            long expectedStart;
            if (!long.TryParse(parts[1], out expectedStart))
            {
                return false;
            }
            string name = process.ProcessName ?? string.Empty;
            long actualStart = process.StartTime.ToUniversalTime().Ticks;
            return !process.HasExited
                && name.IndexOf("powershell", StringComparison.OrdinalIgnoreCase) >= 0
                && actualStart == expectedStart;
        }
        catch
        {
            return false;
        }
    }
}
