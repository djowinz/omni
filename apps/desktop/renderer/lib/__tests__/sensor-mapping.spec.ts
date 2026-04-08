import { describe, it, expect } from 'vitest';
import { sensorSnapshotToMetrics } from '../sensor-mapping';
import type { SensorSnapshot } from '@/generated/SensorSnapshot';

function makeSnapshot(overrides?: Partial<SensorSnapshot>): SensorSnapshot {
  return {
    frame: {
      fps: 144,
      frame_time_ms: 6.9,
      frame_time_avg_ms: 7.0,
      frame_time_1percent_ms: 8.5,
      frame_time_01percent_ms: 12.1,
    },
    cpu: {
      total_usage_percent: 45,
      package_temp_c: 62,
    },
    gpu: {
      usage_percent: 78,
      temp_c: 71,
      core_clock_mhz: 1950,
      mem_clock_mhz: 7800,
      vram_used_mb: 6144,
      vram_total_mb: 8192,
      power_draw_w: 220,
      fan_speed_percent: 55,
    },
    ram: {
      usage_percent: 60,
      used_mb: BigInt(16384),
      total_mb: BigInt(32768),
      frequency_mhz: 3200,
      timing_cl: 16,
      temp_c: 40,
    },
    ...overrides,
  } as SensorSnapshot;
}

describe('sensorSnapshotToMetrics', () => {
  describe('given a complete sensor snapshot', () => {
    const snapshot = makeSnapshot();
    const metrics = sensorSnapshotToMetrics(snapshot);

    it('should map frame metrics', () => {
      expect(metrics.fps).toBe(144);
      expect(metrics['frame-time']).toBe(6.9);
      expect(metrics['frame-time.avg']).toBe(7.0);
      expect(metrics['frame-time.1pct']).toBe(8.5);
      expect(metrics['frame-time.01pct']).toBe(12.1);
    });

    it('should map CPU metrics', () => {
      expect(metrics['cpu.usage']).toBe(45);
      expect(metrics['cpu.temp']).toBe(62);
    });

    it('should map GPU metrics', () => {
      expect(metrics['gpu.usage']).toBe(78);
      expect(metrics['gpu.temp']).toBe(71);
      expect(metrics['gpu.clock']).toBe(1950);
      expect(metrics['gpu.mem-clock']).toBe(7800);
      expect(metrics['gpu.vram.used']).toBe(6144);
      expect(metrics['gpu.vram.total']).toBe(8192);
      expect(metrics['gpu.power']).toBe(220);
      expect(metrics['gpu.fan']).toBe(55);
    });

    it('should map RAM metrics with numeric coercion', () => {
      expect(metrics['ram.usage']).toBe(60);
      expect(metrics['ram.used']).toBe(16384);
      expect(metrics['ram.total']).toBe(32768);
    });
  });

  describe('given RAM values as strings', () => {
    const snapshot = makeSnapshot({
      ram: {
        usage_percent: 50,
        used_mb: '8192' as any,
        total_mb: '16384' as any,
        frequency_mhz: 3200,
        timing_cl: 16,
        temp_c: 40,
      },
    });

    it('should coerce string values to numbers', () => {
      const metrics = sensorSnapshotToMetrics(snapshot);
      expect(metrics['ram.used']).toBe(8192);
      expect(metrics['ram.total']).toBe(16384);
    });
  });
});
