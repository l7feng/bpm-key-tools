Basic Pitch (ICASSP 2022) ONNX model
=====================================

This directory contains `nmp.onnx` and `spec.json` copied from
Spotify's Basic Pitch project (https://github.com/spotify/basic-pitch),
file `basic_pitch/saved_models/icassp_2022/nmp.onnx`.

License: Apache License 2.0 (see the upstream LICENSE for the full text).
Copyright belongs to Spotify AB and the Basic Pitch authors.

Model details: 230,444 bytes, SHA-256
2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec

The model expects raw audio at 22,050 Hz, mono, in windows of 43,844 samples
(~1.99 s), shaped [batch, 43844, 1]. It outputs per-frame note activations,
onset activations and pitch contours over 88 pitches (A0–C8).

This repository only redistributes the weight file to make the optional
"神经网络模型" engine work out of the box; the analysis logic on top of the
model (onset envelope tempo estimation, chroma folding, key templates) is
implemented in `../model_runner.py`.
