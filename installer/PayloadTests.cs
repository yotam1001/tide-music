using System;
using System.IO;
using System.IO.Compression;
using System.Text;
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
    public static int Main() {
        foreach (var mutation in new string[] { "good", "host", "missing", "traversal", "duplicate", "case", "checksum", "partial" }) {
            bool valid = true;
            using (var stream = Fixture(mutation)) using (var zip = new ZipArchive(stream, ZipArchiveMode.Read)) {
                try { Payload.Validate(zip); } catch (InvalidDataException) { valid = false; }
            }
            if (valid != (mutation == "good" || mutation == "host")) throw new Exception("Payload gate regression: " + mutation);
        }
        var unsafePath = Path.Combine(Path.GetTempPath(), "unowned-directory");
        try { Payload.Remove(unsafePath); throw new Exception("Unowned removal was allowed"); } catch (IOException) { }
        Console.WriteLine("PASS: payload checksums, exact file set, traversal, duplicates and confined cleanup");
        if (Program.Mode(new string[0]) != "gui" || Program.Mode(new[] { "--silent" }) != "silent" ||
            Program.Mode(new[] { "--preview" }) != "preview" || Program.Mode(new[] { "--help" }) != "help" ||
            Program.Mode(new[] { "--verify-payload" }) != "verify" ||
            Program.Mode(new[] { "--unknown" }) != "invalid" || Program.Mode(new[] { "--silent", "--preview" }) != "invalid")
            throw new Exception("Command-line routing regression");
        if (Program.SetupVersion() != "0.4.6") throw new Exception("Embedded setup version was not read");
        try { Program.InstallArguments("unused", "1.0\"; exit 0"); throw new Exception("Unsafe version accepted"); }
        catch (ArgumentException) { }
        var fakeRoot = Path.Combine(Path.GetTempPath(), "tide-cli-fixture-" + Guid.NewGuid().ToString("N"));
        try {
            Directory.CreateDirectory(Path.Combine(fakeRoot, "scripts"));
            var script = Path.Combine(fakeRoot, "scripts", "install-windows.ps1");
            File.WriteAllText(script, "param($Mode, $AppVersion)\n$ErrorActionPreference = 'Stop'\nif ($Mode -ne 'Install' -or $AppVersion -ne '0.4.6') { exit 99 }\nGet-FileHash -LiteralPath $PSCommandPath | Out-Null\n1..1000 | ForEach-Object { [Console]::Out.WriteLine('out'); [Console]::Error.WriteLine('err') }\nexit 0\n");
            if (Program.RunUnattended(fakeRoot, "0.4.6") != 0) throw new Exception("Silent setup arguments or stream draining failed");
            File.WriteAllText(script, "param($Mode, $AppVersion)\nexit 23\n");
            if (Program.RunUnattended(fakeRoot, "0.4.6") != 23) throw new Exception("Silent setup lost child failure code");
        } finally { Directory.Delete(fakeRoot, true); }
        Console.WriteLine("PASS: silent setup, embedded version, stream draining and failure exit code"); return 0;
    }
}
