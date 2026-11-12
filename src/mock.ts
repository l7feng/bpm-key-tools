export type ReviewState = "pending" | "confirmed" | "undetermined" | "skipped";

export type Track = {
  id: string;
  /** 真实文件绝对路径；演示数据为 null。 */
  path?: string | null;
  fileName: string;
  durationSec: number;
  sizeBytes: number;
  bpmPrimary: number;
  bpmCandidates: number[];
  bpmConfidence: number;
  keyPrimary: string;
  keyTop2: string[];
  keyConfidence: number;
  lowConfidence: boolean;
  /** 文件里已存在的 BPM（TBPM）标签文本。 */
  bpmExisting?: string | null;
  /** 文件里已存在的调性（TKEY / INITIALKEY）标签文本。 */
  keyExisting?: string | null;
};

/**
 * 演示数据。算法内核尚未接入（路线未拍板），此处为固定假候选，
 * 仅用于验证「候选 → 复核 → 分发写入」这条闸门链路。
 */
export const MOCK_TRACKS: Track[] = [
  {
    id: "demo-001",
    fileName: "雨夜追逐_场景01_v3.wav",
    durationSec: 132.4,
    sizeBytes: 23_363_584,
    bpmPrimary: 120.1,
    bpmCandidates: [120.1, 60.0, 240.2],
    bpmConfidence: 3.42,
    keyPrimary: "C major",
    keyTop2: ["C major", "A minor"],
    keyConfidence: 0.71,
    lowConfidence: false,
  },
  {
    id: "demo-002",
    fileName: "城市晨光_铺底_v1.wav",
    durationSec: 198.7,
    sizeBytes: 35_061_760,
    bpmPrimary: 92.4,
    bpmCandidates: [92.4, 184.8],
    bpmConfidence: 2.61,
    keyPrimary: "Eb major",
    keyTop2: ["Eb major", "Bb major"],
    keyConfidence: 0.58,
    lowConfidence: false,
  },
  {
    id: "demo-003",
    fileName: "机械脉冲_loop_140.wav",
    durationSec: 8.0,
    sizeBytes: 1_411_072,
    bpmPrimary: 140.0,
    bpmCandidates: [140.0, 70.0],
    bpmConfidence: 1.12,
    keyPrimary: "A minor",
    keyTop2: ["A minor", "C major"],
    keyConfidence: 0.39,
    lowConfidence: true,
  },
  {
    id: "demo-004",
    fileName: "环境采样_风与铁皮.wav",
    durationSec: 47.3,
    sizeBytes: 8_347_648,
    bpmPrimary: 0,
    bpmCandidates: [],
    bpmConfidence: 0,
    keyPrimary: "—",
    keyTop2: [],
    keyConfidence: 0,
    lowConfidence: true,
  },
];
