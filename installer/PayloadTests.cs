using System;
using System.IO;
using System.IO.Compression;
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
    [STAThread] public static int Main() {
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
        var plain = new InstallOptions(false, "ignored untrusted input");
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
