using System;
using System.IO;
using System.IO.Compression;
using System.Diagnostics;
using System.Text;
using System.Windows.Forms;
using TideSetup;

internal static class PayloadTests {
    static MemoryStream Fixture(string mutation) {
        var bytes = new MemoryStream();
        using (var zip = new ZipArchive(bytes, ZipArchiveMode.Create, true)) {
            var manifest = new StringBuilder();
            foreach (var name in Payload.Files) {
                if (mutation == "missing" && name == "LICENSE") continue;
                var data = Encoding.UTF8.GetBytes("public fixture, never executed");
                using (var stream = zip.CreateEntry(name).Open()) stream.Write(data, 0, data.Length);
                string hash; using (var stream = new MemoryStream(data)) hash = Payload.Hash(stream);
                if (mutation == "checksum" && name == "LICENSE") hash = new String('0', 64);
                manifest.Append(hash).Append("  ").Append(name).Append('\n');
            }
            if (mutation == "traversal") zip.CreateEntry("../outside");
            if (mutation == "duplicate") zip.CreateEntry("LICENSE");
            if (mutation == "case") zip.CreateEntry("license");
            if (mutation == "partial") manifest.Clear();
            if (mutation == "host") {
                foreach (var name in new string[] { "tide-session-host.exe", "scripts/setup-browser-companion.ps1", "docs/BROWSER_COMPANION.md" }) {
                    var data = Encoding.UTF8.GetBytes("public fixture");
                    using (var stream = zip.CreateEntry(name).Open()) stream.Write(data, 0, data.Length);
                    using (var stream = new MemoryStream(data)) manifest.Append(Payload.Hash(stream)).Append("  ").Append(name).Append('\n');
                }
            }
            using (var writer = new StreamWriter(zip.CreateEntry("SHA256SUMS.txt").Open())) writer.Write(manifest);
        }
        bytes.Position = 0; return bytes;
    }
    [STAThread] public static int Main(string[] args) {
        var fixtureName = Path.GetFileName(System.Reflection.Assembly.GetExecutingAssembly().Location);
        if (fixtureName == "startup-failure.exe") return 37;
        if (fixtureName == "startup-success.exe") { System.Threading.Thread.Sleep(5000); return 0; }
        if (args.Length == 1 && args[0] == "--handoff-child") { System.Threading.Thread.Sleep(1200); return 0; }
        if (args.Length == 1 && args[0] == "--readiness-child") {
            try {
                UpdateOptions.Parse(new[] { "--update-from", Int32.MaxValue.ToString(), "--confirm-ready" }).ConfirmPrepared();
                return 0;
            } catch (OperationCanceledException) { return 23; }
        }
        if (args.Length == 1 && args[0] == "--preflight-failure-child") {
            return UpdateOptions.Parse(new[] { "--update-from", Int32.MaxValue.ToString(), "--confirm-ready" })
                .ReportPreflightFailure(new IOException("Windows uninstall entry mismatch; private-path-and-value")) ? 0 : 1;
        }
        using (var form = new SetupWindow(true)) {
            var chromeControl = (CheckBox)form.Controls["chromeConnection"];
            var idControl = (TextBox)form.Controls["chromeExtensionId"];
            if (chromeControl.Checked || idControl.Enabled) throw new Exception("Chrome registration must require explicit opt-in.");
            chromeControl.Checked = true;
            if (!idControl.Enabled) throw new Exception("Extension ID entry did not enable after opt-in.");
            chromeControl.Checked = false;
            if (idControl.Enabled) throw new Exception("Extension ID entry remained active without opt-in.");
            foreach (Control control in form.Controls) {
                if (control.Left < 0 || control.Top < 0 || control.Right > form.ClientSize.Width || control.Bottom > form.ClientSize.Height)
                    throw new Exception("Installer control is outside its client area: " + control.GetType().Name);
            }
        }
        var script = Path.Combine(Path.GetTempPath(), "fixture folder", "install-windows.ps1");
        if (Payload.AppVersion != "0.1.0-preview.1") throw new Exception("Embedded installer version was lost.");
        foreach (var invalid in new string[][] { new[] { "--update-from", "0" }, new[] { "--update-from", "-1" }, new[] { "--update-from", "1", "extra" }, new[] { "--unknown" }, new[] { "--update-from", Process.GetCurrentProcess().Id.ToString() } }) {
            try { UpdateOptions.Parse(invalid); throw new Exception("Invalid update handoff accepted."); }
            catch (ArgumentException) { }
        }
        if (UpdateOptions.Parse(new string[0]) != null || UpdateOptions.Parse(new[] { "--preview" }) != null)
            throw new Exception("Normal setup unexpectedly enabled automatic installation.");
        var deadParent = UpdateOptions.Parse(new[] { "--update-from", Int32.MaxValue.ToString() });
        deadParent.WaitForPlayer();
        deadParent.WaitForPlayer(); // Retrying setup must not re-identify a reused PID.
        using (var rejected = Process.Start(new ProcessStartInfo {
            FileName = System.Reflection.Assembly.GetExecutingAssembly().Location,
            Arguments = "--preflight-failure-child", UseShellExecute = false, CreateNoWindow = true,
            RedirectStandardOutput = true, RedirectStandardError = true
        })) {
            var result = rejected.StandardOutput.ReadToEndAsync();
            if (!rejected.WaitForExit(10000) || rejected.ExitCode != 0 || result.Result.Trim() != "TIDE_UPDATE_FAILED|ownership")
                throw new Exception("Preflight error must identify its category without private values.");
        }
        foreach (var response in new[] { "COMMIT", "CANCEL" }) {
            using (var prepared = Process.Start(new ProcessStartInfo {
                FileName = System.Reflection.Assembly.GetExecutingAssembly().Location,
                Arguments = "--readiness-child", UseShellExecute = false, CreateNoWindow = true,
                RedirectStandardOutput = true, RedirectStandardInput = true, RedirectStandardError = true
            })) {
                var ready = prepared.StandardOutput.ReadLineAsync();
                if (!ready.Wait(10000) || ready.Result != "TIDE_UPDATE_READY|" + Int32.MaxValue + "|" + Payload.AppVersion)
                    throw new Exception("Setup readiness receipt was missing or incorrect.");
                if (prepared.HasExited) throw new Exception("Setup proceeded before explicit commit.");
                prepared.StandardInput.WriteLine(response); prepared.StandardInput.Flush();
                if (!prepared.WaitForExit(10000) || prepared.ExitCode != (response == "COMMIT" ? 0 : 23))
                    throw new Exception("Setup commit/cancel protocol failed.");
            }
        }
        using (var updateForm = new SetupWindow(true, deadParent)) {
            if (updateForm.ShowInTaskbar) throw new Exception("Automatic updates must not show a setup taskbar window.");
        }
        var handoffRoot = Path.Combine(Path.GetTempPath(), "tide-handoff-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(handoffRoot);
        var childPath = Path.Combine(handoffRoot, "tide-music.exe");
        try {
            File.Copy(System.Reflection.Assembly.GetExecutingAssembly().Location, childPath);
            using (var child = Process.Start(new ProcessStartInfo { FileName = childPath, Arguments = "--handoff-child", UseShellExecute = false, CreateNoWindow = true })) {
                UpdateOptions.Parse(new[] { "--update-from", child.Id.ToString() }).WaitForPlayer();
                if (!child.HasExited || child.ExitCode != 0) throw new Exception("Setup did not wait for normal Tide shutdown.");
            }
        } finally { File.Delete(childPath); Directory.Delete(handoffRoot); }
        var plain = new InstallOptions(false, "ignored untrusted input");
        if (!plain.Arguments(script).Contains("-AppVersion 0.1.0-preview.1")) throw new Exception("Update version not forwarded.");
        if (plain.Chrome || plain.Arguments(script).Contains("-Browser") || !plain.Arguments(script).Contains("-File \"" + script + "\""))
            throw new Exception("Default installer arguments changed browser consent or path quoting.");
        var chrome = new InstallOptions(true, "  ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP  ");
        if (!chrome.Chrome || chrome.ExtensionId != "abcdefghijklmnopabcdefghijklmnop"
            || !chrome.Arguments(script).EndsWith(" -Browser Chrome -ChromiumExtensionId abcdefghijklmnopabcdefghijklmnop", StringComparison.Ordinal))
            throw new Exception("Exact Chrome extension ID was not propagated.");
        foreach (var value in new string[] { null, "", "short", new String('q', 32), new String('a', 31), new String('a', 33), new String('a', 32) + " -Mode Uninstall", "\"; exit" }) {
            try { new InstallOptions(true, value); throw new Exception("Invalid Chrome ID accepted."); }
            catch (ArgumentException) { }
        }
        var scratch = Path.Combine(Path.GetTempPath(), "tide setup process " + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(scratch);
        var failedPlayer = Path.Combine(scratch, "startup-failure.exe");
        var goodPlayer = Path.Combine(scratch, "startup-success.exe");
        File.Copy(System.Reflection.Assembly.GetExecutingAssembly().Location, failedPlayer);
        File.Copy(System.Reflection.Assembly.GetExecutingAssembly().Location, goodPlayer);
        try {
            for (var attempt = 0; attempt < 2; attempt++) {
                try { SetupWindow.StartPlayer(failedPlayer, false); throw new Exception("Early startup exit was treated as a restart success."); }
                catch (IOException error) { if (!error.Message.Contains("exit 37")) throw; }
            }
            using (var running = SetupWindow.StartPlayer(goodPlayer, false)) {
                if (running.HasExited) throw new Exception("Started player was not alive.");
                running.Kill(); running.WaitForExit();
            }
        } finally { File.Delete(failedPlayer); File.Delete(goodPlayer); }
        var previousModules = Environment.GetEnvironmentVariable("PSModulePath");
        try {
            var fixtureScript = Path.Combine(scratch, "fixture.ps1");
            File.WriteAllText(fixtureScript, "param($Mode,$Browser,$ChromiumExtensionId,$AppVersion)\n$ErrorActionPreference='Stop'\nGet-FileHash -LiteralPath $PSCommandPath | Out-Null\nif ($Mode -ne 'Install' -or $Browser -ne 'Chrome' -or $ChromiumExtensionId -ne 'abcdefghijklmnopabcdefghijklmnop' -or $AppVersion -ne '0.1.0-preview.1') { throw 'Argument transfer failed' }\nWrite-Output 'PASS native setup process arguments and system modules'\n");
            Environment.SetEnvironmentVariable("PSModulePath", Path.Combine(scratch, "nonexistent-pwsh-modules"));
            using (var process = Process.Start(chrome.StartInfo(fixtureScript))) {
                var stdout = process.StandardOutput.ReadToEndAsync();
                var stderr = process.StandardError.ReadToEndAsync();
                // Cold PowerShell startup on a busy hosted runner can exceed 15 seconds.
                if (!process.WaitForExit(60000)) { process.Kill(); process.WaitForExit(); throw new Exception("Setup process test timed out."); }
                if (process.ExitCode != 0 || !stdout.Result.Contains("PASS native setup process arguments and system modules"))
                    throw new Exception("Setup process could not use its system modules or arguments: " + stderr.Result);
            }
        } finally {
            Environment.SetEnvironmentVariable("PSModulePath", previousModules);
            File.Delete(Path.Combine(scratch, "fixture.ps1")); Directory.Delete(scratch);
        }
        foreach (var mutation in new string[] { "good", "host", "missing", "traversal", "duplicate", "case", "checksum", "partial" }) {
            bool valid = true;
            using (var stream = Fixture(mutation)) using (var zip = new ZipArchive(stream, ZipArchiveMode.Read)) {
                try { Payload.Validate(zip); } catch (InvalidDataException) { valid = false; }
            }
            if (valid != (mutation == "good" || mutation == "host")) throw new Exception("Payload gate regression: " + mutation);
        }
        var unsafePath = Path.Combine(Path.GetTempPath(), "unowned-directory");
        try { Payload.Remove(unsafePath); throw new Exception("Unowned removal was allowed"); } catch (IOException) { }
        Console.WriteLine("PASS: payload checksums, exact file set, traversal, duplicates and confined cleanup"); return 0;
    }
}
