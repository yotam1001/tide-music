# Windows-only native regression. Uses temporary APPDATA, synthetic headers,
# a fake identity helper, and silent local audio; no browser or network access.
import tempfile,pathlib,os,json,subprocess,sys,struct,wave,argparse,threading,queue
parser=argparse.ArgumentParser(description="Isolated native connection adoption regression")
parser.add_argument("--app",required=True,type=pathlib.Path)
parser.add_argument("--host",required=True,type=pathlib.Path)
args=parser.parse_args()
if sys.platform != "win32": parser.error("This probe requires Windows")
app_exe=args.app.resolve(strict=True);host_exe=args.host.resolve(strict=True)
root=pathlib.Path.cwd()
with tempfile.TemporaryDirectory(prefix='tide-connection-smoke-') as d:
 p=pathlib.Path(d).resolve();(p/'synthetic-connection-fixture').touch()
 helper=p/'fixture.py';helper.write_text('import json,sys,time\nr=json.load(sys.stdin)\na=r["action"]\nif a=="library" and r["headers"].get("x-goog-authuser")=="2": time.sleep(3)\nprint(json.dumps({"type":"session","headers":r["headers"]} if a in ("verify_import", "switchaccount") else {"type":"library","account":"Synthetic account", "playlists":[],"truncated":False} if a=="library" else {"type":"home","cards":[]}))\n')
 headers={'cookie':'__Secure-3PAPISID' + '=' + 'synthetic-only','x-goog-authuser':'1','origin':'https://music.youtube.com'}
 for name,index in [('reconnect',1),('switch',2),('switch-again',3)]:
  h=dict(headers);h['x-goog-authuser']=str(index);h['cookie']='__Secure-3PAPISID' + '=' + 'synthetic-'+name
  (p/(name+'.json')).write_text(json.dumps(h))
 with wave.open(str(p/'silence.wav'),'wb') as w:
  w.setnchannels(1);w.setsampwidth(2);w.setframerate(8000);w.writeframes(b'\0\0'*8000*90)
 env=dict(os.environ,APPDATA=str(p),TIDE_PROVIDER_HELPER=str(helper),TIDE_PROVIDER_PYTHON=sys.executable)
 payload=json.dumps({'version':1,'action':'import_session','headers':headers}).encode()
 proc=subprocess.Popen([str(host_exe),str(p/'manifest.json'),'tide-session@tide-music'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,creationflags=subprocess.CREATE_NO_WINDOW)
 proc.stdin.write(struct.pack('<I',len(payload))+payload);proc.stdin.flush()
 replies=queue.Queue()
 def receive():
  try:
   length=struct.unpack('<I',proc.stdout.read(4))[0];replies.put(json.loads(proc.stdout.read(length)))
  except Exception as error: replies.put(error)
 threading.Thread(target=receive,daemon=True).start()
 try:
  reply=replies.get(timeout=10);assert isinstance(reply,dict) and reply.get('ok');proc.wait(timeout=10)
  assert not proc.stderr.read()
 finally:
  if proc.poll() is None: proc.kill();proc.wait(timeout=5)
  proc.stdin.close()

 app=subprocess.run([str(app_exe),'--smoke-connection',str(p)],env=env,capture_output=True,timeout=65)
 print(app.stdout.decode(errors='replace'));print(app.stderr.decode(errors='replace'));assert app.returncode==0,app.returncode
