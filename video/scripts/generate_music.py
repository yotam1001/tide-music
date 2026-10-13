"""Generate the demo's original, quiet synth bed without external audio assets."""

from pathlib import Path
import wave

import numpy as np


RATE = 44100
DURATION = 31.0
count = int(RATE * DURATION)
left = np.zeros(count, dtype=np.float64)
right = np.zeros(count, dtype=np.float64)


def note(start: float, duration: float, frequency: float, amplitude: float, pan: float = 0.0) -> None:
    first = int(start * RATE)
    length = min(int(duration * RATE), count - first)
    if length <= 0:
        return
    t = np.arange(length) / RATE
    attack = np.minimum(1.0, t / 0.035)
    release = np.minimum(1.0, (duration - t) / 0.30)
    envelope = np.maximum(0.0, np.minimum(attack, release))
    sound = (np.sin(2 * np.pi * frequency * t) + 0.18 * np.sin(2 * np.pi * frequency * 2 * t)) * envelope * amplitude
    left[first:first + length] += sound * (1 - max(0.0, pan))
    right[first:first + length] += sound * (1 + min(0.0, pan))


# Four gentle chord changes. The music is purpose-made for this video.
chords = [
    [110.00, 164.81, 196.00, 261.63],  # Am7
    [87.31, 130.81, 164.81, 220.00],   # Fmaj7
    [130.81, 164.81, 196.00, 246.94],  # Cmaj7
    [98.00, 146.83, 196.00, 246.94],   # G
]
for bar in range(16):
    base = bar * 2.0
    chord = chords[bar % 4]
    for frequency in chord:
        note(base, 2.2, frequency, 0.032)
    for step, index in enumerate([1, 2, 3, 2, 1, 2, 3, 2]):
        note(base + step * 0.25, 0.22, chord[index] * 2, 0.024, -0.15 if step % 2 else 0.15)
    note(base, 0.36, chord[0] / 2, 0.053)

fade_in = np.minimum(1.0, np.arange(count) / (RATE * 0.7))
fade_out = np.minimum(1.0, (count - np.arange(count)) / (RATE * 1.2))
master = np.stack([left, right], axis=1) * np.minimum(fade_in, fade_out)[:, None]
master = np.tanh(master * 1.4) * 0.8
pcm = (master * 32767).astype('<i2')

output = Path(__file__).resolve().parents[1] / 'public' / 'music-bed.wav'
with wave.open(str(output), 'wb') as wav:
    wav.setnchannels(2)
    wav.setsampwidth(2)
    wav.setframerate(RATE)
    wav.writeframes(pcm.tobytes())

print(output)
