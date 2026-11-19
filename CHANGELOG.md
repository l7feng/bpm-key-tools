# 变更记录

> 2026-11-09 建立。按时间倒序记录提交；版本号以 `VERSION` 为准，标签以 `git tag` 为准。建立前的逐版说明未单独保留，回溯以 `git log` 为唯一来源。

| 日期 | 提交 | 说明 |
| --- | --- | --- |
| 2026-11-19 | 0.4.1 | 模型引擎落地可用：入仓 Basic Pitch ONNX（Apache-2.0，230 KB）与 `spec.json`，runner 新增 `basic_pitch` 后端（NN 起音 → 自相关 tempo、NN 音符激活 → chroma → 调性模板），补①音高轴修正（Basic Pitch 是 A0–C8，音级基数 21，此前整体偏 3 个半音）②<70 BPM 倍频折叠 ③起音包络变异系数 <0.08 判稳态；`eval_real_manifest` 支持 `BPM_EVAL_ENGINE=model` 并出 `report-real-model.md`；版本 0.4.1 |
| 2026-11-17 | `9627fa0` | 模型引擎接入口（可选）：`model_engine.rs` 配置驱动外部 runner，`engine/model_runner.py` + `spec.example.json` + `README.md`；界面引擎下拉按 `ready` 灰选；`scripts/build.ps1` 改为检查 `npx tauri build` 退出码与产物时间戳，失败 `exit` 非 0 |
| 2026-11-17 | `6f7034e` | 新增真实素材评测 harness：`eval/real-manifest.json` 清单驱动（有真值 / 负例 / 未标注三档），ignored 测试 `eval_real_manifest` 直接算 BPM 严格·容差·误弃·倍频错误、负例按类型通过率、调性 top-1、耗时中位与最大，并写 `eval/results/report-real.md` |
| 2026-11-17 | `71c8016` | v0.4.0 修订：命令行带文件夹启动（`bpm-key-tool.exe "<目录>"` 自动扫描）；BPM 加「包络偏度 ≥ 0.80」稳态守卫，修掉 4 s 稳态大三和弦被报 156.5 BPM / 0.82 的假阳性 |
| 2026-11-17 | `27066a6` | v0.4.0：本地 DSP 算法内核接入（BPM 自相关 / 调性余弦匹配）、算法候选面板与采用、引擎选择器、批量分析；界面「算法未接入」标识下线 |
| 2026-11-17 | `744f96d` | v0.3.0：试听波形预览、文件夹拖入加载、强调色统一为冷蓝（对齐 SFX Tools） |
| 2026-11-09 | `2b74857` | chore: 棣栨�℃彁浜ゆ湰鍦版簮鐮佸揩鐓э紙BPM/璋冩�ц瘑鍒�宸ュ叿楠ㄦ灦锛� |
