// Windows-only setup shell. Player code and its memory footprint are independent.
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.IO.Compression;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using System.Windows.Forms;

namespace TideSetup {
    internal static class Payload {
        internal static string AppVersion {
            get {
                using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("Tide.Version")) {
                    if (stream == null) throw new InvalidDataException("Installer version is missing.");
                    using (var reader = new StreamReader(stream)) {
                        var value = reader.ReadToEnd().Trim();
                        if (!Regex.IsMatch(value, @"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$"))
                            throw new InvalidDataException("Installer version is invalid.");
                        return value;
                    }
                }
            }
        }
        internal static readonly string[] Files = {
            "tide-music.exe", "LICENSE", "THIRD_PARTY_NOTICES.md", "DEPENDENCIES.md", "START_HERE.md",
            "docs/ACCOUNT_SETUP.md", "scripts/setup-runtime.ps1", "scripts/setup-provider.ps1",
            "helpers/youtube_music.py", "helpers/requirements.txt", "scripts/install-common.ps1",
            "scripts/install-windows.ps1", "Install Tide.cmd", "Uninstall Tide.cmd", "docs/INSTALLATION.md"
        };
        internal static string Hash(Stream stream) {
            using (var sha = SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", "").ToLowerInvariant();
        }
        internal static void Validate(ZipArchive zip) {
            var expected = new HashSet<string>(Files, StringComparer.Ordinal);
            if (zip.GetEntry("tide-session-host.exe") != null) {
                expected.Add("tide-session-host.exe"); expected.Add("scripts/setup-browser-companion.ps1"); expected.Add("docs/BROWSER_COMPANION.md");
            }
            expected.Add("SHA256SUMS.txt");
            var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            long size = 0;
            foreach (var entry in zip.Entries) {
                if (!expected.Contains(entry.FullName) || !seen.Add(entry.FullName) || entry.Length > 96 * 1024 * 1024)
                    throw new InvalidDataException("The installer contains an unexpected or oversized file. Download it again from Tide's GitHub releases.");
                size += entry.Length;
                if (size > 256 * 1024 * 1024) throw new InvalidDataException("The installer payload is too large.");
            }
            if (!seen.SetEquals(expected)) throw new InvalidDataException("The installer payload is incomplete.");
            var hashes = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            var manifest = zip.GetEntry("SHA256SUMS.txt");
            if (manifest.Length > 16384) throw new InvalidDataException("Invalid checksum manifest.");
            using (var reader = new StreamReader(manifest.Open(), Encoding.UTF8)) {
                string line;
                while ((line = reader.ReadLine()) != null) {
                    var match = Regex.Match(line, "^([a-f0-9]{64})  (.+)$");
                    if (!match.Success || match.Groups[2].Value == "SHA256SUMS.txt" || !expected.Contains(match.Groups[2].Value) || hashes.ContainsKey(match.Groups[2].Value))
                        throw new InvalidDataException("Invalid checksum manifest.");
                    hashes.Add(match.Groups[2].Value, match.Groups[1].Value);
                }
            }
            if (hashes.Count != expected.Count - 1) throw new InvalidDataException("Incomplete checksum manifest.");
            foreach (var pair in hashes) using (var stream = zip.GetEntry(pair.Key).Open()) {
                if (Hash(stream) != pair.Value) throw new InvalidDataException("Installer checksum failed. Download Tide again.");
            }
        }
        internal static string Extract() {
            if (String.IsNullOrEmpty(AppVersion)) throw new InvalidDataException("Installer version is missing.");
            var assembly = Assembly.GetExecutingAssembly();
            using (var stream = assembly.GetManifestResourceStream("Tide.Payload"))
            using (var digest = assembly.GetManifestResourceStream("Tide.Payload.sha256")) {
                if (stream == null || digest == null) throw new InvalidDataException("Installer payload missing.");
                string expected;
                using (var reader = new StreamReader(digest)) expected = reader.ReadToEnd().Trim();
                if (Hash(stream) != expected) throw new InvalidDataException("Installer checksum failed. Download Tide again.");
                stream.Position = 0;
                using (var zip = new ZipArchive(stream, ZipArchiveMode.Read)) {
                    Validate(zip);
                    var root = Path.Combine(Path.GetTempPath(), "TideSetup-" + Guid.NewGuid().ToString("N"));
                    AssertPlain(root);
                    Directory.CreateDirectory(root);
                    try {
                        foreach (var entry in zip.Entries) {
                            var path = Path.GetFullPath(Path.Combine(root, entry.FullName));
                            if (!path.StartsWith(root + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("Unsafe payload path.");
                            AssertPlain(path);
                            Directory.CreateDirectory(Path.GetDirectoryName(path));
                            using (var input = entry.Open()) using (var output = new FileStream(path, FileMode.CreateNew)) input.CopyTo(output);
                        }
                        return root;
                    } catch { Remove(root); throw; }
                }
            }
        }
        internal static void AssertPlain(string path) {
            for (var current = Path.GetFullPath(path); current != null; current = Path.GetDirectoryName(current)) {
                if ((File.Exists(current) || Directory.Exists(current)) && (File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0)
                    throw new IOException("Setup cannot use a folder containing a symbolic link or junction.");
            }
        }
        internal static void Remove(string root) {
            // Only this process's random direct child of TEMP can be removed.
            if (Path.GetDirectoryName(root) != Path.GetTempPath().TrimEnd(Path.DirectorySeparatorChar)
                || !Regex.IsMatch(Path.GetFileName(root), "^TideSetup-[a-f0-9]{32}$")) throw new IOException("Invalid temporary setup path.");
            AssertPlain(root);
            if (!Directory.Exists(root)) return;
            RemoveChildren(root);
            Directory.Delete(root);
        }
        static void RemoveChildren(string root) {
            foreach (var path in Directory.GetFileSystemEntries(root)) {
                AssertPlain(path);
                if (Directory.Exists(path)) { RemoveChildren(path); Directory.Delete(path); }
                else File.Delete(path);
            }
        }
    }

    internal sealed class InstallOptions {
        internal readonly string ExtensionId;
        internal bool Chrome { get { return ExtensionId != null; } }
        internal InstallOptions(bool chrome, string extensionId) {
            if (!chrome) return;
            var value = (extensionId ?? "").Trim().ToLowerInvariant();
            if (!Regex.IsMatch(value, "^[a-p]{32}$", RegexOptions.CultureInvariant))
                throw new ArgumentException("Load Tide's companion in Chrome, then paste its exact 32-letter ID from chrome://extensions. This ID is not a password.");
            ExtensionId = value;
        }
        internal string Arguments(string script) {
            return "-NoLogo -NoProfile -ExecutionPolicy Bypass -File \"" + script + "\" -Mode Install"
                + " -AppVersion " + Payload.AppVersion
                + (Chrome ? " -Browser Chrome -ChromiumExtensionId " + ExtensionId : "");
        }
        internal ProcessStartInfo StartInfo(string script, bool preflight = false) {
            var system = Environment.GetFolderPath(Environment.SpecialFolder.System);
            var start = new ProcessStartInfo {
                FileName = Path.Combine(system, "WindowsPowerShell", "v1.0", "powershell.exe"),
                Arguments = Arguments(script) + (preflight ? " -PreflightOnly" : ""), UseShellExecute = false, CreateNoWindow = true,
                RedirectStandardOutput = true, RedirectStandardError = true,
                StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8
            };
            // A setup launched from pwsh must not load incompatible PowerShell 7
            // modules in its Windows PowerShell 5.1 child. Do not edit user env.
            start.EnvironmentVariables["PSModulePath"] = Path.Combine(system, "WindowsPowerShell", "v1.0", "Modules");
            return start;
        }
    }

    internal sealed class UpdateOptions {
        internal readonly int ParentId;
        readonly bool confirmReady;
        bool readySent, parentFinished;
        internal bool RequiresConfirmation { get { return confirmReady; } }
        UpdateOptions(int parentId, bool confirm) { ParentId = parentId; confirmReady = confirm; }
        internal static UpdateOptions Parse(string[] args) {
            if (args.Length == 0 || (args.Length == 1 && (args[0] == "--preview" || args[0] == "--verify-payload"))) return null;
            int id;
            if ((args.Length != 2 && args.Length != 3) || args[0] != "--update-from"
                || (args.Length == 3 && args[2] != "--confirm-ready") || !Int32.TryParse(args[1], out id)
                || id <= 0 || id == Process.GetCurrentProcess().Id) throw new ArgumentException("Invalid update handoff.");
            return new UpdateOptions(id, args.Length == 3);
        }
        internal void ConfirmPrepared() {
            if (confirmReady && !readySent) {
                WriteProtocolLine("TIDE_UPDATE_READY|" + ParentId + "|" + Payload.AppVersion);
                readySent = true;
                // This is an ASCII pipe protocol, independent of the user's
                // console code page and .NET's redirected TextReader encoding.
                var answer = System.Threading.Tasks.Task.Factory.StartNew(delegate {
                    var input = Console.OpenStandardInput();
                    var bytes = new StringBuilder();
                    for (var index = 0; index < 32; index++) {
                        var value = input.ReadByte();
                        if (value < 0) return "";
                        // Framework StreamWriter.AutoFlush can emit a UTF-8
                        // preamble merely by opening redirected StandardInput.
                        if (index == 0 && value == 0xef) {
                            if (input.ReadByte() != 0xbb || input.ReadByte() != 0xbf) return "";
                            continue;
                        }
                        if (value == 10) return bytes.ToString().TrimEnd('\r');
                        if (value > 127) return "";
                        bytes.Append((char)value);
                    }
                    return "";
                });
                if (!answer.Wait(35000) || answer.Result != "COMMIT")
                    throw new OperationCanceledException("Update restart canceled before installation.");
            }
        }
        static void WriteProtocolLine(string line) {
            var bytes = Encoding.ASCII.GetBytes(line + "\n");
            var output = Console.OpenStandardOutput();
            output.Write(bytes, 0, bytes.Length); output.Flush();
        }
        internal bool ReportPreflightFailure(Exception error) {
            if (!confirmReady || readySent) return false;
            var text = error.Message.ToLowerInvariant();
            var category = text.Contains("another tide setup") ? "busy"
                : (text.Contains("uninstall entry") || text.Contains("start menu") || text.Contains("owned tide installation")) ? "ownership"
                : text.Contains("checksum") ? "package" : "preflight";
            WriteProtocolLine("TIDE_UPDATE_FAILED|" + category);
            return true;
        }
        internal void WaitForPlayer() {
            if (parentFinished) return;
            Process parent;
            try { parent = Process.GetProcessById(ParentId); }
            catch (ArgumentException) { parentFinished = true; return; }
            using (parent) {
                try {
                    if (parent.HasExited) { parentFinished = true; return; }
                    if (!String.Equals(parent.ProcessName, "tide-music", StringComparison.OrdinalIgnoreCase)) throw new IOException("The update handoff does not identify Tide.");
                } catch (InvalidOperationException) { parentFinished = true; return; }
                if (!parent.WaitForExit(120000)) throw new IOException("Tide is still closing. Close it before retrying setup.");
                parentFinished = true;
            }
        }
    }

    internal sealed class SetupWindow : Form {
        readonly Label heading = new Label(), description = new Label(), status = new Label();
        readonly Button action = new Button(), close = new Button(), connect = new Button();
        readonly ProgressBar progress = new ProgressBar();
        readonly TextBox errorDetails = new TextBox();
        readonly CheckBox chrome = new CheckBox();
        readonly Label extensionLabel = new Label();
        readonly TextBox extensionId = new TextBox();
        readonly Button guide = new Button();
        readonly BackgroundWorker worker = new BackgroundWorker();
        readonly bool preview;
        readonly UpdateOptions update;
        bool busy, completed, updateStarted, revealUpdateError;
        string installed;
        InstallOptions options;
        // Layout previews must not take keyboard focus from a running game.
        protected override bool ShowWithoutActivation { get { return preview || update != null; } }
        protected override void SetVisibleCore(bool value) {
            if (update != null && !revealUpdateError) {
                if (value && !updateStarted) {
                    updateStarted = true;
                    if (!IsHandleCreated) CreateHandle();
                    BeginInvoke(new Action(Install));
                }
                base.SetVisibleCore(false);
                return;
            }
            base.SetVisibleCore(value);
        }
        void RevealUpdateError() {
            if (update == null) return;
            revealUpdateError = true;
            ShowInTaskbar = true;
            Show();
        }
        readonly string installRoot = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "Tide");
        internal SetupWindow(bool demo, UpdateOptions handoff = null) {
            update = handoff;
            if (update != null) ShowInTaskbar = false;
            preview = demo; Text = "Tide Setup"; ClientSize = new Size(580, 510); MinimumSize = Size; MaximumSize = Size;
            StartPosition = FormStartPosition.CenterScreen; MaximizeBox = false; AutoScaleMode = AutoScaleMode.Dpi;
            BackColor = Color.FromArgb(247, 242, 236); ForeColor = Color.FromArgb(51, 43, 47); Font = new Font("Segoe UI", 10);
            Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath) ?? SystemIcons.Application;
            var brand = new Label { Text = "Tide", Location = new Point(28, 23), Size = new Size(440, 44), Font = new Font("Segoe UI", 26, FontStyle.Bold) };
            var logo = new PictureBox { Image = Icon.ToBitmap(), Location = new Point(505, 27), Size = new Size(40, 40), SizeMode = PictureBoxSizeMode.Zoom };
            heading.SetBounds(30, 90, 520, 32); heading.Font = new Font("Segoe UI", 15, FontStyle.Bold);
            description.SetBounds(30, 132, 520, 84);
            errorDetails.SetBounds(30, 164, 520, 54); errorDetails.Multiline = true; errorDetails.ReadOnly = true;
            errorDetails.ScrollBars = ScrollBars.Vertical; errorDetails.Visible = false; errorDetails.BackColor = BackColor;
            chrome.SetBounds(30, 225, 520, 28); chrome.Text = "Enable Chrome companion connection (optional)";
            chrome.Name = "chromeConnection"; chrome.TabIndex = 0;
            extensionLabel.SetBounds(30, 258, 520, 22); extensionLabel.Text = "Extension ID from chrome://extensions (not a password)"; extensionLabel.Enabled = false;
            extensionId.SetBounds(30, 284, 520, 28); extensionId.MaxLength = 64; extensionId.Enabled = false;
            extensionId.Name = "chromeExtensionId"; extensionId.AccessibleName = "Chrome extension ID"; extensionId.TabIndex = 1;
            chrome.CheckedChanged += delegate { extensionId.Enabled = chrome.Checked; extensionLabel.Enabled = chrome.Checked; };
            guide.SetBounds(30, 326, 280, 32); guide.Text = "Chrome extension setup guide"; guide.FlatStyle = FlatStyle.Flat;
            guide.TabIndex = 2;
            guide.Click += delegate {
                try { Process.Start(new ProcessStartInfo { FileName = "https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md", UseShellExecute = true }); }
                catch (Exception) { MessageBox.Show(this, "Open Tide's GitHub repository and read docs/BROWSER_COMPANION_BETA.md.", "Tide Setup"); }
            };
            status.SetBounds(30, 380, 520, 30); status.Font = new Font("Segoe UI", 9);
            progress.SetBounds(30, 414, 520, 10); progress.Visible = false;
            action.SetBounds(350, 450, 200, 36); action.FlatStyle = FlatStyle.Flat; action.FlatAppearance.BorderSize = 0;
            action.BackColor = Color.FromArgb(139, 36, 63); action.ForeColor = Color.White;
            action.TabIndex = 3;
            close.SetBounds(30, 450, 110, 36); close.Text = "Close"; close.FlatStyle = FlatStyle.Flat;
            close.TabIndex = 5;
            connect.SetBounds(150, 450, 190, 36); connect.Text = "Connect YouTube Music"; connect.FlatStyle = FlatStyle.Flat; connect.Visible = false;
            connect.TabIndex = 4;
            connect.Click += delegate { OpenTide(true); };
            Controls.AddRange(new Control[] { brand, logo, heading, description, status, progress, action, close, connect, errorDetails, chrome, extensionLabel, extensionId, guide });
            heading.Text = "Music ready for your next game";
            description.Text = "Install Tide and its private playback tools. No administrator access needed.\n\nFor Chrome login, load the companion from the guide, then enable its connection below. Existing accounts are kept on update.";
            status.Text = preview ? "Installer preview · no changes will be made" : "Windows x64 · internet required · unsigned preview";
            action.Text = "Install Tide"; action.Click += delegate { if (completed) OpenTide(false); else Install(); };
            close.Click += delegate { Close(); };
            FormClosing += delegate(object sender, FormClosingEventArgs args) { if (busy) { args.Cancel = true; MessageBox.Show(this, "Setup is still working. Please wait for it to finish before closing.", "Tide Setup"); } };
            worker.WorkerReportsProgress = true;
            worker.ProgressChanged += delegate(object sender, ProgressChangedEventArgs args) { progress.Style = ProgressBarStyle.Continuous; progress.Value = Math.Max(0, Math.Min(100, args.ProgressPercentage)); status.Text = (string)args.UserState; };
            worker.DoWork += RunInstall;
            worker.RunWorkerCompleted += delegate(object sender, RunWorkerCompletedEventArgs args) {
                busy = false; action.Enabled = true; close.Enabled = true; guide.Enabled = true;
                if (args.Error != null) {
                    if (args.Error is OperationCanceledException) { Close(); return; }
                    if (update != null && update.ReportPreflightFailure(args.Error)) { Close(); return; }
                    RevealUpdateError();
                    heading.Text = "Setup couldn't finish"; status.Text = "Your saved account and preferences were not removed.";
                    description.Text = "Close Tide if it is running, check your connection, then try again.";
                    description.Height = 28; chrome.Visible = true; chrome.Enabled = true;
                    extensionLabel.Visible = true; extensionId.Visible = true; extensionId.Enabled = chrome.Checked;
                    errorDetails.Text = args.Error.Message; errorDetails.Visible = true; errorDetails.BringToFront();
                    action.Text = "Try again"; progress.Visible = false;
                } else {
                    completed = true; installed = (string)args.Result; progress.Value = 100; heading.Text = "Tide is ready";
                    status.Text = preview ? "Preview only · nothing was installed or registered" : "Find Tide in Start. Remove it through Windows Settings → Apps.";
                    description.Height = 180;
                    description.Text = options.Chrome
                        ? "Chrome's local helper is registered for the extension ID you supplied.\n\nIn Chrome, open YouTube Music and select your account. In Tide's extension, choose Check local connection, then Import selected tab. Click Library in YouTube Music to finish the import.\n\nOpen Tide to use your library. Never share session headers in chat."
                        : "Open Tide to start listening. Existing account connections are preserved.\n\nFor a new Chrome connection, follow the guide, load Tide's extension, then run this installer again with Enable Chrome companion connection selected.\n\nConnect YouTube Music opens Account in Tide. Firefox/Zen automatic connection and public extension-store distribution remain unavailable.";
                    action.Text = "Open Tide"; close.Text = "Finish";
                    connect.Visible = true;
                    if (update != null) OpenTide(false);
                }
            };
        }
        void Install() {
            try { options = new InstallOptions(chrome.Checked, extensionId.Text); }
            catch (ArgumentException error) { MessageBox.Show(this, error.Message, "Chrome extension ID"); extensionId.Focus(); return; }
            busy = true; action.Enabled = false; close.Enabled = false; progress.Visible = true;
            guide.Enabled = false; chrome.Visible = false; extensionLabel.Visible = false; extensionId.Visible = false;
            errorDetails.Visible = false;
            description.Height = 180;
            progress.Style = ProgressBarStyle.Marquee; heading.Text = "Installing Tide";
            description.Text = "Setup is checking the application and preparing its private playback tools. Downloads can take a few minutes.\n\nYour saved account and preferences will stay in place.";
            status.Text = "Preparing setup…"; worker.RunWorkerAsync();
        }
        void RunInstall(object sender, DoWorkEventArgs args) {
            if (preview) { worker.ReportProgress(100, "Preview complete · nothing installed"); args.Result = null; return; }
            string root = null, result = null;
            try {
                root = Payload.Extract();
                var start = options.StartInfo(Path.Combine(root, "scripts", "install-windows.ps1"));
                if (!File.Exists(start.FileName)) throw new IOException("Windows PowerShell is unavailable; setup has not changed your installation.");
                if (update != null) {
                    if (update.RequiresConfirmation && RunSetup(options.StartInfo(Path.Combine(root, "scripts", "install-windows.ps1"), true)) != "TIDE_PREFLIGHT_OK")
                        throw new IOException("Setup did not finish its read-only installation checks.");
                    update.ConfirmPrepared();
                    worker.ReportProgress(0, "Waiting for Tide to close…");
                    update.WaitForPlayer();
                }
                result = RunSetup(start);
                if (result == null || !Path.GetFullPath(result).StartsWith(installRoot + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase) || Path.GetFileName(result) != "tide-music.exe" || !File.Exists(result))
                    throw new IOException("Setup finished without a valid application path. Try setup again.");
                args.Result = result;
            } finally { if (root != null) { try { Payload.Remove(root); } catch (IOException) { /* A locked staging file can be cleared by Windows temp cleanup. */ } } }
        }
        string RunSetup(ProcessStartInfo start) {
                string result = null; var errors = new StringBuilder();
                using (var process = new Process { StartInfo = start }) {
                    process.OutputDataReceived += delegate(object source, DataReceivedEventArgs line) {
                        if (line.Data == null) return;
                        var parts = line.Data.Split(new char[] { '|' }, 3);
                        int percent;
                        if (parts.Length == 3 && parts[0] == "TIDE_PROGRESS" && Int32.TryParse(parts[1], out percent)) worker.ReportProgress(percent, parts[2]);
                        if (line.Data.StartsWith("TIDE_RESULT|", StringComparison.Ordinal)) result = line.Data.Substring(12);
                        if (line.Data == "TIDE_PREFLIGHT_OK") result = line.Data;
                    };
                    process.ErrorDataReceived += delegate(object source, DataReceivedEventArgs line) { if (line.Data != null) lock (errors) { if (errors.Length < 4000) errors.AppendLine(line.Data); } };
                    process.Start(); process.BeginOutputReadLine(); process.BeginErrorReadLine(); process.WaitForExit();
                    if (process.ExitCode != 0) throw new IOException(errors.Length == 0 ? "Installation failed. Check your connection and close Tide before retrying." : errors.ToString());
                }
                return result;
        }
        void OpenTide(bool account) {
            if (preview) { MessageBox.Show(this, "This is a visual preview. No application was installed.", "Tide Setup"); return; }
            try { using (var player = StartPlayer(installed, account)) { } Close(); }
            catch (Exception error) {
                RevealUpdateError();
                heading.Text = "Update installed; Tide couldn't reopen";
                status.Text = "Your new version is installed. Choose Open Tide to retry.";
                action.Text = "Open Tide"; action.Enabled = true;
                errorDetails.Text = error.Message; errorDetails.Visible = true; errorDetails.BringToFront();
            }
        }
        internal static Process StartPlayer(string executable, bool account) {
            var player = Process.Start(new ProcessStartInfo {
                FileName = executable, Arguments = account ? "--connect-account" : "",
                WorkingDirectory = Path.GetDirectoryName(executable), UseShellExecute = false, CreateNoWindow = true
            });
            if (player == null) throw new IOException("Windows did not start Tide. Retry Open Tide.");
            if (player.WaitForExit(1000)) {
                var code = player.ExitCode; player.Dispose();
                throw new IOException("Tide closed during startup (exit " + code + "). Retry Open Tide or open it from Start.");
            }
            return player;
        }
    }
    internal static class Program {
        [STAThread] static int Main(string[] args) {
            try {
                var update = UpdateOptions.Parse(args);
                if (args.Length == 1 && args[0] == "--verify-payload") { var root = Payload.Extract(); Payload.Remove(root); return 0; }
                Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
                Application.Run(new SetupWindow(args.Length == 1 && args[0] == "--preview", update)); return 0;
            } catch (Exception error) {
                if (args.Length > 0) { Console.Error.WriteLine(error.Message); return 1; }
                MessageBox.Show("Tide Setup could not start. Download the installer again.\n\n" + error.Message, "Tide Setup"); return 1;
            }
        }
    }
}
