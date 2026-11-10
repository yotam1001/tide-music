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

namespace GroveSetup {
    internal static class Payload {
        internal static readonly string[] Files = {
            "grove-music.exe", "LICENSE", "THIRD_PARTY_NOTICES.md", "DEPENDENCIES.md", "START_HERE.md",
            "Uninstall Grove.cmd", "docs/BETA.md", "scripts/install-grove.ps1", "scripts/grove-install-common.ps1"
        };
        internal static string Hash(Stream stream) {
            using (var sha = SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", "").ToLowerInvariant();
        }
        internal static void Validate(ZipArchive zip) {
            var expected = new HashSet<string>(Files, StringComparer.Ordinal);
            expected.Add("SHA256SUMS.txt");
            var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            long size = 0;
            foreach (var entry in zip.Entries) {
                if (!expected.Contains(entry.FullName) || !seen.Add(entry.FullName) || entry.Length > 128 * 1024 * 1024)
                    throw new InvalidDataException("The installer contains an unexpected or oversized file. Download the original Grove beta package again.");
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
                if (Hash(stream) != pair.Value) throw new InvalidDataException("Installer checksum failed. Download Grove again.");
            }
        }
        internal static string Extract() {
            var assembly = Assembly.GetExecutingAssembly();
            using (var stream = assembly.GetManifestResourceStream("Grove.Payload"))
            using (var digest = assembly.GetManifestResourceStream("Grove.Payload.sha256")) {
                if (stream == null || digest == null) throw new InvalidDataException("Installer payload missing.");
                string expected;
                using (var reader = new StreamReader(digest)) expected = reader.ReadToEnd().Trim();
                if (Hash(stream) != expected) throw new InvalidDataException("Installer checksum failed. Download Grove again.");
                stream.Position = 0;
                using (var zip = new ZipArchive(stream, ZipArchiveMode.Read)) {
                    Validate(zip);
                    var root = Path.Combine(Path.GetTempPath(), "GroveSetup-" + Guid.NewGuid().ToString("N"));
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
                || !Regex.IsMatch(Path.GetFileName(root), "^GroveSetup-[a-f0-9]{32}$")) throw new IOException("Invalid temporary setup path.");
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

    internal sealed class SetupWindow : Form {
        readonly Label heading = new Label(), description = new Label(), status = new Label();
        readonly Button action = new Button(), close = new Button(), connect = new Button();
        readonly ProgressBar progress = new ProgressBar();
        readonly TextBox errorDetails = new TextBox();
        readonly BackgroundWorker worker = new BackgroundWorker();
        readonly bool preview;
        bool busy, completed;
        string installed;
        readonly string installRoot = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "Grove");
        internal SetupWindow(bool demo) {
            preview = demo; Text = "Grove Setup"; ClientSize = new Size(580, 410); MinimumSize = Size; MaximumSize = Size;
            StartPosition = FormStartPosition.CenterScreen; MaximizeBox = false; AutoScaleMode = AutoScaleMode.Dpi;
            BackColor = Color.FromArgb(20, 22, 23); ForeColor = Color.FromArgb(245, 247, 248); Font = new Font("Segoe UI", 10);
            Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath);
            var brand = new Label { Text = "Grove", Location = new Point(28, 23), Size = new Size(440, 44), Font = new Font("Segoe UI", 26, FontStyle.Bold) };
            var logo = new PictureBox { Image = Icon.ToBitmap(), Location = new Point(505, 27), Size = new Size(40, 40), SizeMode = PictureBoxSizeMode.Zoom };
            heading.SetBounds(30, 90, 520, 32); heading.Font = new Font("Segoe UI", 15, FontStyle.Bold);
            description.SetBounds(30, 132, 520, 140);
            errorDetails.SetBounds(30, 164, 520, 104); errorDetails.Multiline = true; errorDetails.ReadOnly = true;
            errorDetails.ScrollBars = ScrollBars.Vertical; errorDetails.Visible = false; errorDetails.BackColor = BackColor;
            status.SetBounds(30, 280, 520, 30); status.Font = new Font("Segoe UI", 9);
            progress.SetBounds(30, 314, 520, 10); progress.Visible = false;
            action.SetBounds(350, 350, 200, 36); action.FlatStyle = FlatStyle.Flat; action.FlatAppearance.BorderSize = 0;
            action.BackColor = Color.FromArgb(20, 123, 72); action.ForeColor = Color.White;
            close.SetBounds(30, 350, 110, 36); close.Text = "Close"; close.FlatStyle = FlatStyle.Flat;
            connect.SetBounds(150, 350, 190, 36); connect.Text = "Connect Spotify"; connect.FlatStyle = FlatStyle.Flat; connect.Visible = false;
            connect.Click += delegate { OpenGrove(true); };
            Controls.AddRange(new Control[] { brand, logo, heading, description, status, progress, action, close, connect, errorDetails });
            heading.Text = "Your music, in Grove";
            description.Text = "Install Grove for your Windows user. Spotify playback runs inside Grove. Setup downloads verified support for local audio files. No administrator access or Python installation is needed.\n\nConnect Spotify from Account after opening Grove. Spotify Premium is required for streaming. Shared jams are experimental.";
            status.Text = preview ? "Installer preview · no changes will be made" : "Windows x64 · internet required · unsigned private beta";
            action.Text = "Install Grove"; action.Click += delegate { if (completed) OpenGrove(false); else Install(); };
            close.Click += delegate { Close(); };
            FormClosing += delegate(object sender, FormClosingEventArgs args) { if (busy) { args.Cancel = true; MessageBox.Show(this, "Setup is still working. Please wait for it to finish before closing.", "Grove Setup"); } };
            worker.WorkerReportsProgress = true;
            worker.ProgressChanged += delegate(object sender, ProgressChangedEventArgs args) { progress.Style = ProgressBarStyle.Continuous; progress.Value = Math.Max(0, Math.Min(100, args.ProgressPercentage)); status.Text = (string)args.UserState; };
            worker.DoWork += RunInstall;
            worker.RunWorkerCompleted += delegate(object sender, RunWorkerCompletedEventArgs args) {
                busy = false; action.Enabled = true; close.Enabled = true;
                if (args.Error != null) {
                    heading.Text = "Setup couldn't finish"; status.Text = "Your saved account and preferences were not removed.";
                    description.Text = "Close Grove if it is running, check your connection, then try again.";
                    errorDetails.Text = args.Error.Message; errorDetails.Visible = true; errorDetails.BringToFront();
                    action.Text = "Try again"; progress.Visible = false;
                } else {
                    completed = true; installed = (string)args.Result; progress.Value = 100; heading.Text = "Grove is ready";
                    status.Text = "Find Grove in Start. Remove it through Windows Settings → Apps.";
                    description.Text = "Open Grove and choose Account → Connect Spotify, then Enable playback. Approve both requests with the same Spotify account.\n\nShared jams are experimental and currently work on trusted local networks. Read the included beta guide for known limits and feedback instructions.";
                    action.Text = "Open Grove"; close.Text = "Finish";
                    connect.Visible = true;
                }
            };
        }
        void Install() {
            busy = true; action.Enabled = false; close.Enabled = false; progress.Visible = true;
            errorDetails.Visible = false;
            progress.Style = ProgressBarStyle.Marquee; heading.Text = "Installing Grove";
            description.Text = "Setup is checking the application and preparing its private playback tools. Downloads can take a few minutes.\n\nYour saved account and preferences will stay in place.";
            status.Text = "Preparing setup…"; worker.RunWorkerAsync();
        }
        void RunInstall(object sender, DoWorkEventArgs args) {
            if (preview) { worker.ReportProgress(100, "Preview complete · nothing installed"); args.Result = null; return; }
            string root = null, result = null; var errors = new StringBuilder();
            try {
                root = Payload.Extract();
                var start = new ProcessStartInfo {
                    FileName = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), "WindowsPowerShell", "v1.0", "powershell.exe"),
                    Arguments = "-NoLogo -NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(root, "scripts", "install-grove.ps1") + "\" -Mode Install",
                    UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true, RedirectStandardError = true,
                    StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8
                };
                using (var process = new Process { StartInfo = start }) {
                    process.OutputDataReceived += delegate(object source, DataReceivedEventArgs line) {
                        if (line.Data == null) return;
                        var parts = line.Data.Split(new char[] { '|' }, 3);
                        int percent;
                        if (parts.Length == 3 && parts[0] == "GROVE_PROGRESS" && Int32.TryParse(parts[1], out percent)) worker.ReportProgress(percent, parts[2]);
                        if (line.Data.StartsWith("GROVE_RESULT|", StringComparison.Ordinal)) result = line.Data.Substring(13);
                    };
                    process.ErrorDataReceived += delegate(object source, DataReceivedEventArgs line) { if (line.Data != null) lock (errors) { if (errors.Length < 4000) errors.AppendLine(line.Data); } };
                    process.Start(); process.BeginOutputReadLine(); process.BeginErrorReadLine(); process.WaitForExit();
                    if (process.ExitCode != 0) throw new IOException(errors.Length == 0 ? "Installation failed. Check your connection and close Grove before retrying." : errors.ToString());
                }
                if (result == null || !Path.GetFullPath(result).StartsWith(installRoot + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase) || Path.GetFileName(result) != "grove-music.exe" || !File.Exists(result))
                    throw new IOException("Setup finished without a valid application path. Try setup again.");
                args.Result = result;
            } finally { if (root != null) { try { Payload.Remove(root); } catch (IOException) { /* A locked staging file can be cleared by Windows temp cleanup. */ } } }
        }
        void OpenGrove(bool account) {
            if (preview) { MessageBox.Show(this, "This is a visual preview. No application was installed.", "Grove Setup"); return; }
            try { Process.Start(new ProcessStartInfo { FileName = installed, Arguments = account ? "--connect-account" : "", WorkingDirectory = Path.GetDirectoryName(installed), UseShellExecute = true }); Close(); }
            catch (Exception) { MessageBox.Show(this, "Grove could not open. Try launching it from the Start menu.", "Grove Setup"); }
        }
    }
    internal static class Program {
        [STAThread] static int Main(string[] args) {
            try {
                if (args.Length == 1 && args[0] == "--verify-payload") { var root = Payload.Extract(); Payload.Remove(root); return 0; }
                Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
                Application.Run(new SetupWindow(args.Length == 1 && args[0] == "--preview")); return 0;
            } catch (Exception error) {
                if (args.Length > 0) { Console.Error.WriteLine(error.Message); return 1; }
                MessageBox.Show("Grove Setup could not start. Download the installer again.\n\n" + error.Message, "Grove Setup"); return 1;
            }
        }
    }
}
