import sys, librosa, numpy as np
y, sr = librosa.load(sys.argv[1], sr=22050, mono=True)
tempo, beats = librosa.beat.beat_track(y=y, sr=sr, units='time')
print('tempo', tempo)
bt = np.array(beats)
print('first beats', np.round(bt[:12],3))
# energy per 1s to find drops
rms = librosa.feature.rms(y=y, hop_length=sr//4)[0]
t = np.arange(len(rms))/4
for i in range(0, len(rms), 4):
    print(f"{t[i]:6.1f}s {'#'*int(rms[i:i+4].mean()*120)}")
np.savetxt(sys.argv[2], bt, fmt='%.4f')
