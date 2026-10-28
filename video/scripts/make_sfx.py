# Synthesizes the ad's sound effects (deterministic, seeded).
import numpy as np, wave, sys, os
SR = 48000; out = sys.argv[1]; rng = np.random.default_rng(7)
def t(d): return np.arange(int(SR*d))/SR
def save(name, x, gain=0.9):
    x = x / (np.max(np.abs(x)) + 1e-9) * gain
    st = np.stack([x, x], 1)
    with wave.open(os.path.join(out, name), 'wb') as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((st*32767).astype(np.int16).tobytes())
def lp(x, a):  # one-pole low-pass
    y = np.zeros_like(x); s = 0.0
    for i, v in enumerate(x): s += a*(v-s); y[i] = s
    return y
def bp_sweep(x, f0, f1):  # crude band-pass via two one-pole filters with swept coefficient
    n = len(x); f = np.geomspace(f0, f1, n); a = 1-np.exp(-2*np.pi*f/SR)
    y = np.zeros(n); s1 = s2 = 0.0
    for i in range(n): s1 += a[i]*(x[i]-s1); s2 += a[i]*(s1-s2); y[i] = s1-s2
    return y
# glitch: stuttered bitcrushed noise + square blips
d = t(0.36); x = rng.standard_normal(len(d))
x = np.round(x*3)/3; gate = (np.floor(d*55) % 2 == 0).astype(float)
x = x*gate*0.6 + np.sign(np.sin(2*np.pi*(180+600*d)*d))*0.4*gate
save('glitch.wav', x*np.exp(-d*5), 0.7)
# crack: sharp transient + glassy partials
d = t(0.9); x = rng.standard_normal(len(d))*np.exp(-d*60)
for f in (2350, 3170, 4410, 5230, 6870): x += 0.25*np.sin(2*np.pi*f*d)*np.exp(-d*(6+f/900))*rng.uniform(.6,1)
save('crack.wav', x, 0.8)
# whoosh: swept band-passed noise, swell then fade
d = t(0.9); x = bp_sweep(rng.standard_normal(len(d)), 300, 5000)
env = np.sin(np.pi*np.clip(d/0.9,0,1))**2
save('whoosh.wav', x*env, 0.6)
# tick: short soft click
d = t(0.09); x = np.sin(2*np.pi*2400*d)*np.exp(-d*90) + 0.4*np.sin(2*np.pi*1200*d)*np.exp(-d*60)
save('tick.wav', x, 0.5)
# thump: low pitched-down sine
d = t(0.35); f = 120*np.exp(-d*9)+45; x = np.sin(2*np.pi*np.cumsum(f)/SR)*np.exp(-d*9)
save('thump.wav', x, 0.85)
# impact: sub boom + filtered noise tail
d = t(2.2); f = 70*np.exp(-d*5)+38; x = np.sin(2*np.pi*np.cumsum(f)/SR)*np.exp(-d*2.2)
x += 0.35*lp(rng.standard_normal(len(d)), 0.08)*np.exp(-d*2.8)
save('impact.wav', x, 0.9)
# swell: reverse-style noise rising into a point
d = t(1.2); x = bp_sweep(rng.standard_normal(len(d)), 800, 9000)*(d/1.2)**3
save('swell.wav', x, 0.5)
print('ok')
