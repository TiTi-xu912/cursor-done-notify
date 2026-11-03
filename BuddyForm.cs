using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Drawing.Text;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace CursorDoneBuddy
{
    public class BuddyForm : Form
    {
        const int WmNcHitTest = 0x84;
        const int HtTransparent = -1;
        const int HtClient = 1;

        float _scale = 1f;
        string _phase = "idle";
        string _accent = "blue";
        string _headline = "";
        string _activity = "";
        string _detail = "";
        string _meta = "";
        string _hint = "";
        string _history = "";
        string _pose = "idle";
        string _queuedPose;
        PendingBubble _pending;
        int _animClock;
        int _poseSince;
        Dictionary<string, PoseClip> _clips = new Dictionary<string, PoseClip>();
        bool _expanded;
        bool _shown;
        bool _dragged;
        bool _downOnClose;
        int _tick;
        int _celebrate;
        float _start = -40f;
        float _sweep = 360f;
        float _bob;
        float _lean;
        bool _poked;
        int _pokeUntil;
        RectangleF _characterRect;
        Rectangle _closeRect;
        List<BoardHit> _boardHits = new List<BoardHit>();
        byte[] _alpha;
        int _alphaW;
        int _alphaH;
        Timer _timer;
        Point _grab;
        int _lastDragX;
        bool _blitFailed;

        Font _fontHeadline;
        Font _fontMeta;
        Font _fontActivity;
        Font _fontDetail;
        Font _fontHint;
        StringFormat _single;
        StringFormat _wrap;

        public BuddyForm()
        {
            FormBorderStyle = FormBorderStyle.None;
            ShowInTaskbar = false;
            TopMost = true;
            StartPosition = FormStartPosition.Manual;
            AutoScaleMode = AutoScaleMode.None;
            BackColor = Color.Black;
            SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.UserPaint | ControlStyles.OptimizedDoubleBuffer, true);
            using (Graphics graphics = Graphics.FromHwnd(IntPtr.Zero))
            {
                _scale = graphics.DpiX / 96f;
            }
            if (_scale < 1f) _scale = 1f;
            if (_scale > 2.5f) _scale = 2.5f;
            BuildFonts();
            _single = new StringFormat();
            _single.Trimming = StringTrimming.EllipsisCharacter;
            _single.FormatFlags = StringFormatFlags.NoWrap;
            _wrap = new StringFormat();
            _wrap.Trimming = StringTrimming.EllipsisCharacter;
            ClientSize = new Size((int)Math.Round(Px(360)), (int)Math.Round(Px(168)));
        }

        public string PositionPath { get; set; }
        public string SpritePath { get; set; }
        Bitmap _sprites;
        public string ActivatedSessionKey { get; private set; }
        public event EventHandler SessionActivated;
        public event EventHandler DismissRequested;

        public static void EnableDpi()
        {
            try { SetProcessDPIAware(); } catch { }
        }

        public void SetContent(string phase, string accent, string headline, string activity, string detail, string meta, string hint, string history, string pose)
        {
            string nextPhase = string.IsNullOrEmpty(phase) ? "idle" : phase;
            string nextPose = string.IsNullOrEmpty(pose) ? "idle" : pose;
            bool poseChange = nextPose != _pose;
            bool urgent = nextPose == "done" || nextPose == "error";
            int shown = _poseSince == 0 ? 1000 : Environment.TickCount - _poseSince;
            bool queue = poseChange && !urgent && shown >= 0 && shown < 560 && !string.IsNullOrEmpty(_headline);
            if (queue)
            {
                _queuedPose = nextPose;
                _pending = new PendingBubble();
                _pending.Phase = nextPhase;
                _pending.Accent = string.IsNullOrEmpty(accent) ? "blue" : accent;
                _pending.Headline = headline ?? "";
                _pending.Activity = activity ?? "";
                _pending.Detail = detail ?? "";
                _pending.Meta = meta ?? "";
                _pending.Hint = hint ?? "";
                _pending.History = history ?? "";
                _pending.Pose = nextPose;
                return;
            }
            _queuedPose = null;
            _pending = null;
            ApplyContent(nextPhase, accent, headline, activity, detail, meta, hint, history);
            if (poseChange) StartPose(nextPose);
            if (_shown) PresentFrame();
            else EnsureSize();
        }

        public Bitmap RenderPreview(string phase, string accent, string headline, string activity, string detail, string meta, string hint, string history, float ringStart, float ringSweep, string pose)
        {
            SetContent(phase, accent, headline, activity, detail, meta, hint, history, pose);
            _start = ringStart;
            _sweep = ringSweep;
            _celebrate = (phase == "done" || phase == "settling") && accent == "green" ? 10 : 0;
            return Render();
        }

        public void ClampToScreen()
        {
            Screen screen = Screen.FromRectangle(Bounds);
            if (screen == null) screen = Screen.PrimaryScreen;
            Rectangle area = screen.WorkingArea;
            int x = Left;
            int y = Top;
            if (Width > area.Width) x = area.Left;
            else
            {
                if (Right > area.Right - 8) x = area.Right - Width - 8;
                if (x < area.Left + 8) x = area.Left + 8;
            }
            if (Height > area.Height) y = area.Top;
            else
            {
                if (Bottom > area.Bottom - 8) y = area.Bottom - Height - 8;
                if (y < area.Top + 8) y = area.Top + 8;
            }
            Location = new Point(x, y);
        }

        protected override bool ShowWithoutActivation
        {
            get { return true; }
        }

        protected override CreateParams CreateParams
        {
            get
            {
                CreateParams cp = base.CreateParams;
                cp.ExStyle |= 0x00080000; // WS_EX_LAYERED
                cp.ExStyle |= 0x00000080; // WS_EX_TOOLWINDOW
                cp.ExStyle |= 0x08000000; // WS_EX_NOACTIVATE
                return cp;
            }
        }

        protected override void OnShown(EventArgs e)
        {
            base.OnShown(e);
            _shown = true;
            PresentFrame();
            _timer = new Timer();
            _timer.Interval = 33;
            _timer.Tick += OnTick;
            _animClock = Environment.TickCount;
            _poseSince = _animClock;
            _timer.Start();
        }

        protected override void OnFormClosed(FormClosedEventArgs e)
        {
            if (_timer != null)
            {
                _timer.Stop();
                _timer.Dispose();
                _timer = null;
            }
            if (_sprites != null)
            {
                _sprites.Dispose();
                _sprites = null;
            }
            DisposeClips();
            DisposeFonts();
            base.OnFormClosed(e);
        }

        protected override void WndProc(ref Message m)
        {
            if (m.Msg == WmNcHitTest)
            {
                int lp = m.LParam.ToInt32();
                int sx = (short)(lp & 0xFFFF);
                int sy = (short)((lp >> 16) & 0xFFFF);
                Point client = PointToClient(new Point(sx, sy));
                if (AlphaAt(client.X, client.Y) < 16)
                {
                    m.Result = (IntPtr)HtTransparent;
                    return;
                }
                m.Result = (IntPtr)HtClient;
                return;
            }
            base.WndProc(ref m);
        }

        protected override void OnMouseDown(MouseEventArgs e)
        {
            _grab = e.Location;
            _lastDragX = MousePosition.X;
            _dragged = false;
            _downOnClose = _closeRect.Contains(e.Location);
            base.OnMouseDown(e);
        }

        protected override void OnMouseMove(MouseEventArgs e)
        {
            if (e.Button == MouseButtons.Left && !_downOnClose)
            {
                Point screen = MousePosition;
                int nx = screen.X - _grab.X;
                int ny = screen.Y - _grab.Y;
                if (Math.Abs(nx - Left) + Math.Abs(ny - Top) > 4) _dragged = true;
                if (_dragged)
                {
                    int dx = MousePosition.X - _lastDragX;
                    _lastDragX = MousePosition.X;
                    _lean = Math.Max(-16f, Math.Min(16f, _lean * 0.45f + dx * 0.35f));
                    Location = new Point(nx, ny);
                    if (_shown) PresentFrame();
                }
            }
            Cursor = _closeRect.Contains(e.Location) || BoardKeyAt(e.Location) != null || HitCharacter(e.Location) ? Cursors.Hand : Cursors.SizeAll;
            base.OnMouseMove(e);
        }

        protected override void OnMouseUp(MouseEventArgs e)
        {
            if (_downOnClose)
            {
                RaiseDismiss();
            }
            else if (_dragged)
            {
                ClampToScreen();
                SavePosition();
            }
            else
            {
                string rowKey = BoardKeyAt(e.Location);
                if (!string.IsNullOrEmpty(rowKey))
                {
                    ActivatedSessionKey = rowKey;
                    EventHandler activated = SessionActivated;
                    if (activated != null) activated(this, EventArgs.Empty);
                }
                else if (HitCharacter(e.Location))
                {
                    Poke();
                }
                else if (_phase == "done" || _phase == "settling")
                {
                    RaiseDismiss();
                }
                else
                {
                    _expanded = !_expanded;
                    PresentFrame();
                }
            }
            _downOnClose = false;
            base.OnMouseUp(e);
        }

        void OnTick(object sender, EventArgs e)
        {
            _tick++;
            bool draw = false;
            bool animated = ClipFrameCount(_pose) > 1;
            if (_phase == "running")
            {
                _start = (_start + 7.2f) % 360f;
                _sweep = 76f;
                _bob = animated ? 0f : (float)Math.Sin(_tick * 0.28) * _scale * 2.1f;
                draw = true;
            }
            else
            {
                _bob = 0f;
                if (_sweep < 360f)
                {
                    _sweep = Math.Min(360f, _sweep + 30f);
                    draw = true;
                }
                if (_celebrate > 0)
                {
                    _celebrate--;
                    draw = true;
                }
            }
            if (_poked && Environment.TickCount >= _pokeUntil)
            {
                _poked = false;
                draw = true;
            }
            if (Math.Abs(_lean) > 0.4f)
            {
                _lean *= 0.72f;
                draw = true;
            }
            else if (_lean != 0f)
            {
                _lean = 0f;
                draw = true;
            }
            if (AdvanceAnimations()) draw = true;
            if (draw) PresentFrame();
        }

        void RaiseDismiss()
        {
            EventHandler handler = DismissRequested;
            if (handler != null) handler(this, EventArgs.Empty);
        }

        void SavePosition()
        {
            if (string.IsNullOrEmpty(PositionPath)) return;
            try
            {
                System.IO.File.WriteAllText(PositionPath, Left.ToString() + "," + Top.ToString());
            }
            catch { }
        }

        void EnsureSize()
        {
            using (Bitmap scratch = new Bitmap(8, 8, PixelFormat.Format32bppArgb))
            using (Graphics graphics = Graphics.FromImage(scratch))
            {
                Spot layout = Compute(graphics);
                if (ClientSize.Width != layout.Width || ClientSize.Height != layout.Height)
                {
                    ClientSize = new Size(layout.Width, layout.Height);
                }
            }
        }

        void PresentFrame()
        {
            if (!_shown || IsDisposed) return;
            Bitmap frame = Render();
            try
            {
                if (ClientSize.Width != frame.Width || ClientSize.Height != frame.Height)
                {
                    int oldHeight = Height;
                    ClientSize = new Size(frame.Width, frame.Height);
                    if (Height != oldHeight)
                    {
                        Top -= (Height - oldHeight);
                        ClampToScreen();
                    }
                }
                Blit(frame);
            }
            finally
            {
                frame.Dispose();
            }
        }

        Bitmap Render()
        {
            Spot layout;
            Bitmap bmp;
            using (Bitmap scratch = new Bitmap(8, 8, PixelFormat.Format32bppArgb))
            using (Graphics measure = Graphics.FromImage(scratch))
            {
                layout = Compute(measure);
            }
            bmp = new Bitmap(layout.Width, layout.Height, PixelFormat.Format32bppArgb);
            using (Graphics g = Graphics.FromImage(bmp))
            {
                g.SmoothingMode = SmoothingMode.AntiAlias;
                g.PixelOffsetMode = PixelOffsetMode.HighQuality;
                g.TextRenderingHint = TextRenderingHint.AntiAliasGridFit;
                g.Clear(Color.Transparent);
                DrawBubble(g, layout);
                DrawCharacter(g, layout);
            }
            CacheAlpha(bmp);
            _closeRect = Rectangle.Round(layout.Close);
            return bmp;
        }

        void DrawBubble(Graphics g, Spot layout)
        {
            _boardHits.Clear();
            RectangleF bubble = layout.Bubble;
            float radius = Px(18);
            using (GraphicsPath path = Rounded(bubble, radius))
            using (SolidBrush fill = new SolidBrush(Color.FromArgb(248, 255, 250, 244)))
            using (Pen pen = new Pen(Color.FromArgb(255, 232, 214, 190), Math.Max(1f, Px(1.4f))))
            {
                g.FillPath(fill, path);
                g.DrawPath(pen, path);
            }

            Color accent = AccentColor(_accent, _phase);
            float dot = Px(8);
            using (SolidBrush dotBrush = new SolidBrush(accent))
            {
                g.FillEllipse(dotBrush, layout.TextX, layout.Headline.Y + (layout.Headline.Height - dot) / 2f, dot, dot);
            }

            using (SolidBrush title = new SolidBrush(Color.FromArgb(255, 44, 36, 22)))
            using (SolidBrush body = new SolidBrush(Color.FromArgb(255, 63, 52, 40)))
            using (SolidBrush muted = new SolidBrush(Color.FromArgb(255, 122, 110, 96)))
            using (SolidBrush faint = new SolidBrush(Color.FromArgb(255, 156, 146, 132)))
            {
                RectangleF headline = layout.Headline;
                headline.X += dot + Px(6);
                headline.Width -= dot + Px(6);
                g.DrawString(_headline, _fontHeadline, title, headline, _single);
                g.DrawString(_meta, _fontMeta, faint, layout.Meta, _single);
                g.DrawString(_activity, _fontActivity, body, layout.Activity, _single);
                if (layout.HasDetail)
                {
                    g.DrawString(_detail, _fontDetail, muted, layout.Detail, _wrap);
                }
                if (layout.HasHistory)
                {
                    using (Pen line = new Pen(Color.FromArgb(255, 236, 226, 210), Math.Max(1f, Px(1f))))
                    {
                        g.DrawLine(line, layout.TextX, layout.History.Y - Px(5), layout.TextX + layout.TextW, layout.History.Y - Px(5));
                    }
                    if (IsBoard(_history)) DrawBoard(g, layout.History, _history);
                    else
                    {
                        _boardHits.Clear();
                        g.DrawString(_history, _fontHint, muted, layout.History, _wrap);
                    }
                }
                g.DrawString(_hint, _fontHint, faint, layout.Hint, _single);
            }

            RectangleF close = layout.Close;
            using (SolidBrush closeFill = new SolidBrush(Color.FromArgb(255, 244, 232, 216)))
            using (Pen closePen = new Pen(Color.FromArgb(255, 120, 96, 72), Math.Max(1.2f, Px(1.4f))))
            {
                closePen.StartCap = LineCap.Round;
                closePen.EndCap = LineCap.Round;
                g.FillEllipse(closeFill, close);
                float pad = close.Width * 0.32f;
                g.DrawLine(closePen, close.X + pad, close.Y + pad, close.Right - pad, close.Bottom - pad);
                g.DrawLine(closePen, close.Right - pad, close.Y + pad, close.X + pad, close.Bottom - pad);
            }
        }

        void DrawCharacter(Graphics g, Spot layout)
        {
            float lean = _lean;
            float cx = layout.BallCx + lean;
            float cy = layout.BallCy + (_phase == "running" && !_poked ? _bob * 0.25f : 0f);
            float spriteW = layout.SpriteW > 1f ? layout.SpriteW : layout.BodyR * 2f;
            float spriteH = layout.SpriteH > 1f ? layout.SpriteH : layout.BodyR * 2f;
            _characterRect = new RectangleF(cx - spriteW / 2f, cy - spriteH / 2f, spriteW, spriteH);

            using (SolidBrush shadow = new SolidBrush(Color.FromArgb(40, 120, 78, 36)))
            {
                g.FillEllipse(shadow, cx - spriteW * 0.22f, cy + spriteH * 0.36f, spriteW * 0.44f, spriteH * 0.07f);
            }

            RectangleF dest = _characterRect;
            GraphicsState saved = g.Save();
            if (Math.Abs(lean) > 0.6f)
            {
                float pivotY = cy + spriteH * 0.42f;
                g.TranslateTransform(cx, pivotY);
                g.RotateTransform(lean * 0.45f);
                g.TranslateTransform(-cx, -pivotY);
            }
            InterpolationMode previous = g.InterpolationMode;
            g.InterpolationMode = InterpolationMode.HighQualityBicubic;
            DrawSprite(g, CurrentFrame(), dest, 1f);
            g.InterpolationMode = previous;
            g.Restore(saved);
        }

        bool HitCharacter(Point point)
        {
            return _characterRect.Contains(point) && AlphaAt(point.X, point.Y) >= 16;
        }

        void Poke()
        {
            _poked = true;
            _pokeUntil = Environment.TickCount + 420;
            PoseClip idle = LoadPose("idle");
            if (idle != null && idle.Frames != null && idle.Frames.Length > 1)
            {
                idle.Index = 1;
                idle.Wait = 0;
            }
            if (_shown) PresentFrame();
        }

        void DrawSprite(Graphics g, Bitmap frame, RectangleF dest, float opacity)
        {
            if (frame == null || opacity <= 0.01f) return;
            if (opacity >= 0.99f)
            {
                g.DrawImage(frame, dest);
                return;
            }
            float[][] matrix = new float[][] {
                new float[] { 1f, 0f, 0f, 0f, 0f },
                new float[] { 0f, 1f, 0f, 0f, 0f },
                new float[] { 0f, 0f, 1f, 0f, 0f },
                new float[] { 0f, 0f, 0f, opacity, 0f },
                new float[] { 0f, 0f, 0f, 0f, 1f }
            };
            using (ImageAttributes attr = new ImageAttributes())
            {
                attr.SetColorMatrix(new ColorMatrix(matrix), ColorMatrixFlag.Default, ColorAdjustType.Bitmap);
                Rectangle rect = Rectangle.Round(dest);
                g.DrawImage(frame, rect, 0, 0, frame.Width, frame.Height, GraphicsUnit.Pixel, attr);
            }
        }

        Bitmap CurrentFrame()
        {
            if (_poked && Environment.TickCount < _pokeUntil)
            {
                PoseClip idle = LoadPose("idle");
                if (idle != null && idle.Frames != null && idle.Frames.Length > 1) return idle.Frames[1];
            }
            return FrameOf(_pose);
        }

        Bitmap FrameOf(string pose)
        {
            PoseClip clip = LoadPose(pose);
            if (clip == null || clip.Frames == null || clip.Frames.Length == 0) return null;
            int index = clip.Index;
            if (index < 0 || index >= clip.Frames.Length) index = 0;
            return clip.Frames[index];
        }

        int ClipFrameCount(string pose)
        {
            PoseClip clip = LoadPose(pose);
            if (clip == null || clip.Frames == null) return 0;
            return clip.Frames.Length;
        }

        bool AdvanceAnimations()
        {
            int now = Environment.TickCount;
            int dt = now - _animClock;
            _animClock = now;
            if (dt < 0 || dt > 250) dt = 33;
            bool draw = false;
            if (!string.IsNullOrEmpty(_queuedPose))
            {
                int shown = Environment.TickCount - _poseSince;
                if (shown >= 560 || _queuedPose == "done" || _queuedPose == "error")
                {
                    _queuedPose = null;
                    FlushPending();
                    draw = true;
                }
            }
            bool poked = _poked && Environment.TickCount < _pokeUntil;
            if (_poked && !poked)
            {
                _poked = false;
                draw = true;
            }
            if (!poked && AdvanceClip(_pose, dt)) draw = true;
            return draw;
        }

        void ApplyContent(string phase, string accent, string headline, string activity, string detail, string meta, string hint, string history)
        {
            string nextPhase = string.IsNullOrEmpty(phase) ? "idle" : phase;
            bool changed = nextPhase != _phase;
            _phase = nextPhase;
            _accent = string.IsNullOrEmpty(accent) ? "blue" : accent;
            _headline = headline ?? "";
            _activity = activity ?? "";
            _detail = detail ?? "";
            _meta = meta ?? "";
            _hint = hint ?? "";
            _history = history ?? "";
            if (changed)
            {
                if (_phase == "running") _sweep = 76f;
                if (_phase == "idle") _sweep = 360f;
                if ((_phase == "done" || _phase == "settling") && _accent == "green") _celebrate = 16;
            }
        }

        void FlushPending()
        {
            PendingBubble pending = _pending;
            _pending = null;
            if (pending == null) return;
            ApplyContent(pending.Phase, pending.Accent, pending.Headline, pending.Activity, pending.Detail, pending.Meta, pending.Hint, pending.History);
            if (pending.Pose != _pose) StartPose(pending.Pose);
        }

        void StartPose(string nextPose)
        {
            _pose = nextPose;
            _poseSince = Environment.TickCount;
            PoseClip entered = LoadPose(_pose);
            if (entered != null)
            {
                entered.Index = 0;
                entered.Wait = 0;
            }
        }

        bool AdvanceClip(string pose, int dt)
        {
            PoseClip clip = LoadPose(pose);
            if (clip == null || clip.Frames == null || clip.Frames.Length <= 1) return false;
            clip.Wait += dt;
            int delay = 100;
            if (clip.DelayMs != null && clip.Index >= 0 && clip.Index < clip.DelayMs.Length)
            {
                delay = clip.DelayMs[clip.Index];
            }
            if (delay < 40) delay = 40;
            if (clip.Wait < delay) return false;
            clip.Wait -= delay;
            clip.Index = (clip.Index + 1) % clip.Frames.Length;
            return true;
        }

        PoseClip LoadPose(string pose)
        {
            if (string.IsNullOrEmpty(pose)) pose = "idle";
            PoseClip cached;
            if (_clips.TryGetValue(pose, out cached)) return cached;
            PoseClip clip = null;
            string dir = PoseDirectory();
            if (!string.IsNullOrEmpty(dir))
            {
                string gif = Path.Combine(dir, pose + ".gif");
                string strip = Path.Combine(dir, pose + ".png");
                clip = LoadSequence(dir, pose);
                if (clip == null && File.Exists(strip)) clip = LoadStrip(strip);
                if (clip == null && File.Exists(gif)) clip = LoadGif(gif);
            }
            if (clip == null) clip = LoadSheetPose(pose);
            if (clip == null) return null;
            _clips[pose] = clip;
            return clip;
        }

        string PoseDirectory()
        {
            if (string.IsNullOrEmpty(SpritePath)) return null;
            string dir = Path.GetDirectoryName(SpritePath);
            if (string.IsNullOrEmpty(dir)) return null;
            return Path.Combine(dir, "poses");
        }

        PoseClip LoadSequence(string dir, string pose)
        {
            try
            {
                var frames = new System.Collections.Generic.List<Bitmap>();
                for (int i = 0; i < 12; i++)
                {
                    string path = Path.Combine(dir, pose + "-" + i.ToString() + ".png");
                    if (!File.Exists(path)) break;
                    using (Bitmap loaded = new Bitmap(path))
                    {
                        Bitmap copy = new Bitmap(loaded.Width, loaded.Height, PixelFormat.Format32bppArgb);
                        using (Graphics g = Graphics.FromImage(copy))
                        {
                            g.DrawImage(loaded, 0, 0, loaded.Width, loaded.Height);
                        }
                        frames.Add(copy);
                    }
                }
                if (frames.Count < 2) 
                {
                    for (int i = 0; i < frames.Count; i++) frames[i].Dispose();
                    return null;
                }
                int[] delays = new int[frames.Count];
                for (int i = 0; i < delays.Length; i++) delays[i] = 160;
                if (pose == "idle" && delays.Length >= 2)
                {
                    delays[0] = 420;
                    delays[1] = 90;
                    if (delays.Length > 2) delays[2] = 360;
                    if (delays.Length > 3) delays[3] = 420;
                }
                if (pose == "terminal")
                {
                    for (int i = 0; i < delays.Length; i++) delays[i] = (i % 2 == 0) ? 240 : 120;
                }
                PoseClip clip = new PoseClip();
                clip.Frames = frames.ToArray();
                clip.DelayMs = delays;
                return clip;
            }
            catch
            {
                return null;
            }
        }

        PoseClip LoadGif(string path)
        {
            Image image = null;
            try
            {
                image = Image.FromFile(path);
                FrameDimension dim = new FrameDimension(image.FrameDimensionsList[0]);
                int count = image.GetFrameCount(dim);
                if (count < 1) return null;
                byte[] raw = null;
                try
                {
                    PropertyItem item = image.GetPropertyItem(0x5100);
                    if (item != null) raw = item.Value;
                }
                catch { }
                Bitmap[] frames = new Bitmap[count];
                int[] delays = new int[count];
                for (int i = 0; i < count; i++)
                {
                    image.SelectActiveFrame(dim, i);
                    Bitmap frame = new Bitmap(image.Width, image.Height, PixelFormat.Format32bppArgb);
                    using (Graphics g = Graphics.FromImage(frame))
                    {
                        g.Clear(Color.Transparent);
                        g.DrawImage(image, 0, 0, image.Width, image.Height);
                    }
                    KeyBlack(frame);
                    frames[i] = frame;
                    int cs = 10;
                    if (raw != null && raw.Length >= (i + 1) * 4)
                    {
                        cs = raw[i * 4] | (raw[i * 4 + 1] << 8);
                    }
                    if (cs < 2) cs = 10;
                    delays[i] = cs * 10;
                }
                PoseClip clip = new PoseClip();
                clip.Frames = frames;
                clip.DelayMs = delays;
                return clip;
            }
            catch
            {
                return null;
            }
            finally
            {
                if (image != null) image.Dispose();
            }
        }

        PoseClip LoadStrip(string path)
        {
            try
            {
                using (Bitmap sheet = new Bitmap(path))
                {
                    int count = 8;
                    if (sheet.Width < count) return null;
                    int fw = sheet.Width / count;
                    Bitmap[] frames = new Bitmap[count];
                    int[] delays = new int[count];
                    for (int i = 0; i < count; i++)
                    {
                        Bitmap frame = new Bitmap(fw, sheet.Height, PixelFormat.Format32bppArgb);
                        using (Graphics g = Graphics.FromImage(frame))
                        {
                            g.DrawImage(sheet, new Rectangle(0, 0, fw, sheet.Height), new Rectangle(i * fw, 0, fw, sheet.Height), GraphicsUnit.Pixel);
                        }
                        frames[i] = frame;
                        delays[i] = 90;
                    }
                    PoseClip clip = new PoseClip();
                    clip.Frames = frames;
                    clip.DelayMs = delays;
                    return clip;
                }
            }
            catch
            {
                return null;
            }
        }

        PoseClip LoadSheetPose(string pose)
        {
            EnsureSprites();
            if (_sprites == null) return null;
            int fw = _sprites.Width / 8;
            if (fw < 1) return null;
            int index = PoseFrame(pose);
            Bitmap frame = new Bitmap(fw, _sprites.Height, PixelFormat.Format32bppArgb);
            using (Graphics g = Graphics.FromImage(frame))
            {
                g.DrawImage(_sprites, new Rectangle(0, 0, fw, _sprites.Height), new Rectangle(index * fw, 0, fw, _sprites.Height), GraphicsUnit.Pixel);
            }
            PoseClip clip = new PoseClip();
            clip.Frames = new Bitmap[] { frame };
            clip.DelayMs = new int[] { 100 };
            return clip;
        }

        static void KeyBlack(Bitmap frame)
        {
            Color corner = frame.GetPixel(0, 0);
            if (corner.A < 250) return;
            if (corner.R > 24 || corner.G > 24 || corner.B > 24) return;
            int w = frame.Width;
            int h = frame.Height;
            int thresh = 14;
            bool[] bg = new bool[w * h];
            int[] stack = new int[w * h];
            int top = 0;
            BitmapData data = frame.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
            try
            {
                int stride = data.Stride;
                byte[] buf = new byte[stride * h];
                Marshal.Copy(data.Scan0, buf, 0, buf.Length);
                Action<int, int> push = null;
                push = delegate(int x, int y)
                {
                    if (x < 0 || y < 0 || x >= w || y >= h) return;
                    int i = y * w + x;
                    if (bg[i]) return;
                    int p = y * stride + x * 4;
                    if (buf[p] <= thresh && buf[p + 1] <= thresh && buf[p + 2] <= thresh)
                    {
                        bg[i] = true;
                        stack[top++] = i;
                    }
                };
                for (int x = 0; x < w; x++)
                {
                    push(x, 0);
                    push(x, h - 1);
                }
                for (int y = 0; y < h; y++)
                {
                    push(0, y);
                    push(w - 1, y);
                }
                while (top > 0)
                {
                    int i = stack[--top];
                    int x = i % w;
                    int y = i / w;
                    push(x - 1, y);
                    push(x + 1, y);
                    push(x, y - 1);
                    push(x, y + 1);
                }
                for (int i = 0; i < bg.Length; i++)
                {
                    if (!bg[i]) continue;
                    int p = (i / w) * stride + (i % w) * 4;
                    buf[p] = 0;
                    buf[p + 1] = 0;
                    buf[p + 2] = 0;
                    buf[p + 3] = 0;
                }
                Marshal.Copy(buf, 0, data.Scan0, buf.Length);
            }
            finally
            {
                frame.UnlockBits(data);
            }
        }

        void DisposeClips()
        {
            foreach (KeyValuePair<string, PoseClip> pair in _clips)
            {
                PoseClip clip = pair.Value;
                if (clip == null || clip.Frames == null) continue;
                for (int i = 0; i < clip.Frames.Length; i++)
                {
                    if (clip.Frames[i] != null) clip.Frames[i].Dispose();
                }
            }
            _clips.Clear();
        }

        static bool IsBoard(string text)
        {
            if (string.IsNullOrEmpty(text)) return false;
            return text.IndexOf("run|") >= 0 || text.IndexOf("wait|") >= 0 || text.IndexOf("done|") >= 0 || text.IndexOf("bad|") >= 0 || text.IndexOf("warn|") >= 0;
        }

        static int CountBoardLines(string text)
        {
            if (string.IsNullOrEmpty(text)) return 0;
            int count = 1;
            for (int i = 0; i < text.Length; i++)
            {
                if (text[i] == '\n') count++;
            }
            if (count > 8) count = 8;
            return count;
        }

        void DrawBoard(Graphics g, RectangleF area, string text)
        {
            _boardHits.Clear();
            string[] lines = text.Split('\n');
            float row = Px(18);
            float y = area.Y;
            int shown = 0;
            for (int i = 0; i < lines.Length && shown < 8; i++)
            {
                string line = lines[i];
                if (string.IsNullOrEmpty(line)) continue;
                string mark = "run";
                string key = "";
                string body = line;
                int bar = line.IndexOf('|');
                if (bar > 0)
                {
                    mark = line.Substring(0, bar);
                    string rest = line.Substring(bar + 1);
                    int bar2 = rest.IndexOf('|');
                    if (bar2 >= 0)
                    {
                        key = rest.Substring(0, bar2);
                        body = rest.Substring(bar2 + 1);
                    }
                    else
                    {
                        body = rest;
                    }
                }
                if (!string.IsNullOrEmpty(key))
                {
                    BoardHit hit = new BoardHit();
                    hit.Key = key;
                    hit.Rect = new RectangleF(area.X, y, area.Width, row);
                    _boardHits.Add(hit);
                }
                Color color = Color.FromArgb(255, 47, 111, 235);
                if (mark == "done" || mark == "wait") color = Color.FromArgb(255, 34, 176, 98);
                else if (mark == "bad") color = Color.FromArgb(255, 220, 64, 64);
                else if (mark == "warn") color = Color.FromArgb(255, 224, 138, 30);
                float dot = Px(7);
                using (SolidBrush dotBrush = new SolidBrush(color))
                using (SolidBrush ink = new SolidBrush(Color.FromArgb(255, 63, 52, 40)))
                {
                    g.FillEllipse(dotBrush, area.X, y + (row - dot) / 2f, dot, dot);
                    RectangleF lineRect = new RectangleF(area.X + dot + Px(6), y, Math.Max(1f, area.Width - dot - Px(6)), row);
                    g.DrawString(body, _fontHint, ink, lineRect, _single);
                }
                y += row;
                shown++;
            }
        }

        string BoardKeyAt(Point point)
        {
            for (int i = 0; i < _boardHits.Count; i++)
            {
                if (_boardHits[i].Rect.Contains(point)) return _boardHits[i].Key;
            }
            return null;
        }

        static int PoseFrame(string pose)
        {
            switch (pose)
            {
                case "think": return 1;
                case "read": return 2;
                case "write": return 3;
                case "terminal": return 4;
                case "search": return 5;
                case "done": return 6;
                case "error": return 7;
                default: return 0;
            }
        }

        void EnsureSprites()
        {
            if (_sprites != null || string.IsNullOrEmpty(SpritePath) || !File.Exists(SpritePath)) return;
            _sprites = new Bitmap(SpritePath);
        }


        Spot Compute(Graphics g)
        {
            float margin = Px(12);
            float bubbleW = Px(292);
            float textPadL = Px(14);
            float textPadR = Px(30);
            float textW = bubbleW - textPadL - textPadR;
            float textX = margin + textPadL;
            float y = 0f;
            float blockTop = Px(12);
            y = blockTop;
            float headlineH = Px(22);
            float metaH = string.IsNullOrEmpty(_meta) ? 0f : Px(16);
            float activityH = string.IsNullOrEmpty(_activity) ? 0f : Px(20);
            float detailH = 0f;
            bool hasDetail = !string.IsNullOrEmpty(_detail);
            if (hasDetail)
            {
                SizeF measured = g.MeasureString(_detail, _fontDetail, (int)textW, _wrap);
                detailH = Math.Min(Px(36), measured.Height);
            }
            float historyH = 0f;
            bool board = IsBoard(_history);
            bool hasHistory = board || (_expanded && !string.IsNullOrEmpty(_history) && _phase == "running");
            if (hasHistory)
            {
                if (board)
                {
                    int lines = CountBoardLines(_history);
                    historyH = lines * Px(18) + Px(8);
                }
                else
                {
                    SizeF measuredHistory = g.MeasureString(_history, _fontHint, (int)textW, _wrap);
                    historyH = Math.Min(Px(68), measuredHistory.Height + Px(8));
                }
            }
            float hintH = string.IsNullOrEmpty(_hint) ? 0f : Px(16);
            float textH = headlineH;
            if (metaH > 0) textH += Px(2) + metaH;
            if (activityH > 0) textH += Px(3) + activityH;
            if (detailH > 0) textH += Px(2) + detailH;
            if (historyH > 0) textH += Px(8) + historyH;
            if (hintH > 0) textH += Px(4) + hintH;

            float bubbleH = Math.Max(Px(108), textH + Px(24));
            EnsureSprites();
            float spriteH = Px(188);
            float aspect = 357f / 420f;
            PoseClip layoutClip = LoadPose(string.IsNullOrEmpty(_pose) ? "idle" : _pose);
            if (layoutClip != null && layoutClip.Frames != null && layoutClip.Frames.Length > 0 && layoutClip.Frames[0] != null && layoutClip.Frames[0].Height > 0)
            {
                aspect = (float)layoutClip.Frames[0].Width / layoutClip.Frames[0].Height;
            }
            else if (_sprites != null && _sprites.Height > 0)
            {
                aspect = (_sprites.Width / 8f) / _sprites.Height;
            }
            float spriteW = spriteH * aspect;
            float contentH = Math.Max(bubbleH, spriteH);
            int width = (int)Math.Ceiling(margin + bubbleW + Px(4) + spriteW + margin);
            int height = (int)Math.Ceiling(margin + contentH + margin + Px(6));

            float bubbleY = margin + (contentH - bubbleH) / 2f;
            RectangleF bubble = new RectangleF(margin, bubbleY, bubbleW, bubbleH);
            float ballCx = margin + bubbleW + Px(4) + spriteW / 2f;
            float ballCy = margin + contentH / 2f;

            float cursor = bubbleY + Px(12);
            RectangleF headline = new RectangleF(textX, cursor, textW, headlineH);
            cursor += headlineH + Px(2);
            RectangleF meta = new RectangleF(textX, cursor, textW, metaH);
            if (metaH > 0) cursor += metaH + Px(3);
            RectangleF activity = new RectangleF(textX, cursor, textW, activityH);
            if (activityH > 0) cursor += activityH + Px(2);
            RectangleF detail = new RectangleF(textX, cursor, textW, detailH);
            if (detailH > 0) cursor += detailH;
            RectangleF history = new RectangleF(textX, cursor + Px(8), textW, Math.Max(0f, historyH - Px(8)));
            if (historyH > 0) cursor += historyH;
            RectangleF hint = new RectangleF(textX, cursor + Px(4), textW, hintH);
            RectangleF close = new RectangleF(bubble.Right - Px(24), bubble.Top + Px(8), Px(16), Px(16));

            Spot spot = new Spot();
            spot.Width = width;
            spot.Height = height;
            spot.Bubble = bubble;
            spot.Close = close;
            spot.BallCx = ballCx;
            spot.BallCy = ballCy;
            spot.BodyR = spriteH / 2f;
            spot.SpriteW = spriteW;
            spot.SpriteH = spriteH;
            spot.TextX = textX;
            spot.TextW = textW;
            spot.Headline = headline;
            spot.Meta = meta;
            spot.Activity = activity;
            spot.Detail = detail;
            spot.Hint = hint;
            spot.History = history;
            spot.HasDetail = hasDetail;
            spot.HasHistory = hasHistory;
            return spot;
        }

        void CacheAlpha(Bitmap bmp)
        {
            BitmapData data = bmp.LockBits(new Rectangle(0, 0, bmp.Width, bmp.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            try
            {
                _alphaW = bmp.Width;
                _alphaH = bmp.Height;
                if (_alpha == null || _alpha.Length != _alphaW * _alphaH)
                {
                    _alpha = new byte[_alphaW * _alphaH];
                }
                int stride = data.Stride;
                byte[] row = new byte[Math.Abs(stride)];
                for (int y = 0; y < bmp.Height; y++)
                {
                    IntPtr src = new IntPtr(data.Scan0.ToInt64() + y * stride);
                    Marshal.Copy(src, row, 0, row.Length);
                    for (int x = 0; x < bmp.Width; x++)
                    {
                        _alpha[y * _alphaW + x] = row[x * 4 + 3];
                    }
                }
            }
            finally
            {
                bmp.UnlockBits(data);
            }
        }

        int AlphaAt(int x, int y)
        {
            if (_alpha == null || x < 0 || y < 0 || x >= _alphaW || y >= _alphaH) return 0;
            return _alpha[y * _alphaW + x];
        }

        void Blit(Bitmap bitmap)
        {
            IntPtr screenDc = GetDC(IntPtr.Zero);
            IntPtr memDc = CreateCompatibleDC(screenDc);
            IntPtr dib = IntPtr.Zero;
            IntPtr old = IntPtr.Zero;
            try
            {
                BitmapInfo info = new BitmapInfo();
                info.biSize = Marshal.SizeOf(typeof(BitmapInfo));
                info.biWidth = bitmap.Width;
                info.biHeight = -bitmap.Height;
                info.biPlanes = 1;
                info.biBitCount = 32;
                info.biCompression = 0;
                IntPtr bits;
                dib = CreateDIBSection(screenDc, ref info, 0, out bits, IntPtr.Zero, 0);
                if (dib == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());

                BitmapData data = bitmap.LockBits(new Rectangle(0, 0, bitmap.Width, bitmap.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
                try
                {
                    int width = bitmap.Width;
                    int height = bitmap.Height;
                    int srcStride = data.Stride;
                    byte[] row = new byte[Math.Abs(srcStride)];
                    for (int y = 0; y < height; y++)
                    {
                        Marshal.Copy(new IntPtr(data.Scan0.ToInt64() + y * srcStride), row, 0, row.Length);
                        for (int x = 0; x < width; x++)
                        {
                            int i = x * 4;
                            byte b = row[i];
                            byte gg = row[i + 1];
                            byte r = row[i + 2];
                            byte a = row[i + 3];
                            row[i] = (byte)(b * a / 255);
                            row[i + 1] = (byte)(gg * a / 255);
                            row[i + 2] = (byte)(r * a / 255);
                        }
                        Marshal.Copy(row, 0, new IntPtr(bits.ToInt64() + y * width * 4), width * 4);
                    }
                }
                finally
                {
                    bitmap.UnlockBits(data);
                }

                old = SelectObject(memDc, dib);
                SizeInfo size = new SizeInfo();
                size.cx = bitmap.Width;
                size.cy = bitmap.Height;
                PointInfo src = new PointInfo();
                PointInfo dst = new PointInfo();
                dst.x = Left;
                dst.y = Top;
                BlendFunction blend = new BlendFunction();
                blend.BlendOp = 0;
                blend.BlendFlags = 0;
                blend.SourceConstantAlpha = 255;
                blend.AlphaFormat = 1;
                bool ok = UpdateLayeredWindow(Handle, screenDc, ref dst, ref size, memDc, ref src, 0, ref blend, 2);
                if (!ok && !_blitFailed)
                {
                    _blitFailed = true;
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                }
            }
            finally
            {
                if (old != IntPtr.Zero) SelectObject(memDc, old);
                if (dib != IntPtr.Zero) DeleteObject(dib);
                DeleteDC(memDc);
                ReleaseDC(IntPtr.Zero, screenDc);
            }
        }

        void BuildFonts()
        {
            _fontHeadline = MakeFont(Px(16), FontStyle.Bold);
            _fontActivity = MakeFont(Px(13), FontStyle.Bold);
            _fontMeta = MakeFont(Px(11), FontStyle.Regular);
            _fontDetail = MakeFont(Px(12), FontStyle.Regular);
            _fontHint = MakeFont(Px(11), FontStyle.Regular);
        }

        void DisposeFonts()
        {
            if (_fontHeadline != null) _fontHeadline.Dispose();
            if (_fontActivity != null) _fontActivity.Dispose();
            if (_fontMeta != null) _fontMeta.Dispose();
            if (_fontDetail != null) _fontDetail.Dispose();
            if (_fontHint != null) _fontHint.Dispose();
            if (_single != null) _single.Dispose();
            if (_wrap != null) _wrap.Dispose();
        }

        static Font MakeFont(float px, FontStyle style)
        {
            string[] names = new string[] { "Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI" };
            for (int i = 0; i < names.Length; i++)
            {
                try { return new Font(names[i], px, style, GraphicsUnit.Pixel); }
                catch { }
            }
            return new Font(FontFamily.GenericSansSerif, px, style, GraphicsUnit.Pixel);
        }

        static Color AccentColor(string accent, string phase)
        {
            if (phase == "idle") return Color.FromArgb(255, 176, 164, 146);
            if (accent == "green") return Color.FromArgb(255, 34, 176, 98);
            if (accent == "orange") return Color.FromArgb(255, 224, 138, 30);
            if (accent == "red") return Color.FromArgb(255, 220, 64, 64);
            return Color.FromArgb(255, 47, 111, 235);
        }

        static GraphicsPath Rounded(RectangleF rect, float radius)
        {
            GraphicsPath path = new GraphicsPath();
            float d = Math.Min(radius * 2f, Math.Min(rect.Width, rect.Height));
            path.AddArc(rect.X, rect.Y, d, d, 180, 90);
            path.AddArc(rect.Right - d, rect.Y, d, d, 270, 90);
            path.AddArc(rect.Right - d, rect.Bottom - d, d, d, 0, 90);
            path.AddArc(rect.X, rect.Bottom - d, d, d, 90, 90);
            path.CloseFigure();
            return path;
        }

        float Px(float value)
        {
            return value * _scale;
        }

        [DllImport("user32.dll")]
        static extern bool SetProcessDPIAware();

        [DllImport("user32.dll", SetLastError = true)]
        static extern bool UpdateLayeredWindow(IntPtr hwnd, IntPtr hdcDst, ref PointInfo pptDst, ref SizeInfo psize, IntPtr hdcSrc, ref PointInfo pptSrc, int crKey, ref BlendFunction pblend, int dwFlags);

        [DllImport("user32.dll")]
        static extern IntPtr GetDC(IntPtr hwnd);

        [DllImport("user32.dll")]
        static extern int ReleaseDC(IntPtr hwnd, IntPtr hdc);

        [DllImport("gdi32.dll", SetLastError = true)]
        static extern IntPtr CreateCompatibleDC(IntPtr hdc);

        [DllImport("gdi32.dll")]
        static extern bool DeleteDC(IntPtr hdc);

        [DllImport("gdi32.dll")]
        static extern IntPtr SelectObject(IntPtr hdc, IntPtr obj);

        [DllImport("gdi32.dll")]
        static extern bool DeleteObject(IntPtr obj);

        [DllImport("gdi32.dll", SetLastError = true)]
        static extern IntPtr CreateDIBSection(IntPtr hdc, ref BitmapInfo pbmi, uint usage, out IntPtr bits, IntPtr section, uint offset);

        [StructLayout(LayoutKind.Sequential)]
        struct PointInfo
        {
            public int x;
            public int y;
        }

        [StructLayout(LayoutKind.Sequential)]
        struct SizeInfo
        {
            public int cx;
            public int cy;
        }

        [StructLayout(LayoutKind.Sequential)]
        struct BlendFunction
        {
            public byte BlendOp;
            public byte BlendFlags;
            public byte SourceConstantAlpha;
            public byte AlphaFormat;
        }

        [StructLayout(LayoutKind.Sequential)]
        struct BitmapInfo
        {
            public int biSize;
            public int biWidth;
            public int biHeight;
            public short biPlanes;
            public short biBitCount;
            public int biCompression;
            public int biSizeImage;
            public int biXPelsPerMeter;
            public int biYPelsPerMeter;
            public int biClrUsed;
            public int biClrImportant;
        }

        class BoardHit
        {
            public string Key;
            public RectangleF Rect;
        }

        class PendingBubble
        {
            public string Phase;
            public string Accent;
            public string Headline;
            public string Activity;
            public string Detail;
            public string Meta;
            public string Hint;
            public string History;
            public string Pose;
        }

        class PoseClip
        {
            public Bitmap[] Frames;
            public int[] DelayMs;
            public int Index;
            public int Wait;
        }

        class Spot
        {
            public int Width;
            public int Height;
            public RectangleF Bubble;
            public RectangleF Close;
            public float BallCx;
            public float BallCy;
            public float BodyR;
            public float SpriteW;
            public float SpriteH;
            public float TextX;
            public float TextW;
            public RectangleF Headline;
            public RectangleF Meta;
            public RectangleF Activity;
            public RectangleF Detail;
            public RectangleF Hint;
            public RectangleF History;
            public bool HasDetail;
            public bool HasHistory;
        }
    }
}
