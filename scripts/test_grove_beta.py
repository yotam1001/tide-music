"""Grove package gates and isolated Windows install lifecycle; no real registration or accounts."""
import hashlib
import importlib.util
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parent.parent
def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
gate = load("grove_package", ROOT / "scripts/package-grove-beta.py")
original = load("tide_install_tests", ROOT / "scripts/test_windows_install.py")
HARNESS = original.HARNESS.replace("TIDE", "GROVE").replace("Tide", "Grove").replace("tide", "grove")
HARNESS = HARNESS.replace("org.grovemusic.windows.preview", "org.grovemusic.windows.beta").replace("0.1.0-preview", "0.1.0-beta.1")
HARNESS = HARNESS.replace("Install-Grove $package $root $links 'Chrome' ('a' * 32)", "Install-Grove $package $root $links 'None' ''")
HARNESS = re.sub(r" -or \$global:Registry\['Chrome'\] -ne \$(?:release|previous|current)", "", HARNESS)

class PackageTests(unittest.TestCase):
    def fixture(self, path, extra=None, corrupt=False):
        payload = {name: b"public synthetic fixture" for name in gate.FILES}
        hashes = "\n".join(hashlib.sha256(data).hexdigest()+"  "+name for name,data in payload.items())+"\n"
        if corrupt: payload["LICENSE"] = b"changed after checksum"
        with zipfile.ZipFile(path, "w") as archive:
            for name,data in payload.items(): archive.writestr(name,data)
            archive.writestr("SHA256SUMS.txt",hashes)
            if extra: archive.writestr(extra,b"unowned fixture")
    def test_exact_payload_and_corruption(self):
        with tempfile.TemporaryDirectory(prefix="grove-package-test-") as temp:
            for name,extra,corrupt in [("good",None,False),("traversal","../outside",False),("extra","credentials.json",False),("case","license",False),("checksum",None,True)]:
                path=Path(temp)/(name+".zip");self.fixture(path,extra,corrupt)
                if name=="good": gate.check(path)
                else:
                    with self.assertRaises(ValueError): gate.check(path)

@unittest.skipUnless(os.name == "nt", "Windows installer")
class InstallerTests(unittest.TestCase):
    def test_isolated_lifecycle(self):
        scenarios = ["fresh-remove","upgrade","preserve-extra","dependency-failure-first","dependency-failure-update","shortcut-failure","busy","unknown-root","shortcut-conflict","changed-shortcut-removal","corrupt-package","traversal-record","lock","download","archive-paths","app-entry-write-failure-first","app-entry-write-failure-update","app-entry-marker-failure","app-entry-version-update","app-entry-migration","app-entry-new-race","protocol"]
        shell=shutil.which("pwsh") or "powershell.exe"
        for scenario in scenarios:
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory(prefix="grove-install-test-") as temp:
                fixture=Path(temp);package=fixture/"package";lines=[]
                for name in gate.FILES:
                    path=package/name;path.parent.mkdir(parents=True,exist_ok=True)
                    data=b"public fixture, never executed";path.write_bytes(data)
                    lines.append(hashlib.sha256(data).hexdigest()+"  "+name)
                (package/"SHA256SUMS.txt").write_text("\n".join(lines),encoding="utf-8")
                if scenario=="archive-paths":
                    for name,entries in {"traversal":["../outside.txt"],"rooted":["/outside.txt"],"duplicate":["same.txt","SAME.txt"]}.items():
                        with zipfile.ZipFile(fixture/(name+".zip"),"w") as archive:
                            for entry in entries: archive.writestr(entry,b"fixture")
                script=fixture/"test.ps1";script.write_text(HARNESS,encoding="utf-8-sig")
                result=subprocess.run([shell,"-NoProfile","-File",str(script),str(ROOT/"scripts/grove-install-common.ps1"),str(fixture),scenario],capture_output=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr.decode(errors="replace"))
                self.assertIn(b"PASS isolated installer lifecycle",result.stdout)
    def test_bootstrap_payload_gate(self):
        compiler=Path(os.environ["SystemRoot"])/"Microsoft.NET/Framework64/v4.0.30319/csc.exe"
        with tempfile.TemporaryDirectory(prefix="grove-bootstrap-test-") as temp:
            output=Path(temp)/"tests.exe"
            result=subprocess.run([str(compiler),"/nologo","/target:exe","/main:PayloadTests","/warnaserror+","/out:"+str(output),"/reference:System.Windows.Forms.dll","/reference:System.Drawing.dll","/reference:System.IO.Compression.dll","/reference:System.IO.Compression.FileSystem.dll",str(ROOT/"installer/GroveSetup.cs"),str(ROOT/"installer/GrovePayloadTests.cs")],capture_output=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stdout.decode(errors="replace"))
            result=subprocess.run([str(output)],capture_output=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr.decode(errors="replace"))

if __name__=="__main__": unittest.main()
