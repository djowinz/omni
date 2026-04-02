import type { SensorSnapshot } from '@/src/generated/SensorSnapshot';
import type { MetricValues } from '@/types/omni';

/** Map a SensorSnapshot (ts-rs generated from Rust) to the frontend MetricValues type. */
export function sensorSnapshotToMetrics(snapshot: SensorSnapshot): Partial<MetricValues> {
  return {
    fps: snapshot.frame.fps,
    frametime: snapshot.frame.frame_time_ms,
    'frame.1pct': snapshot.frame.frame_time_1percent_ms,
    'gpu.usage': snapshot.gpu.usage_percent,
    'gpu.temp': snapshot.gpu.temp_c,
    'gpu.clock': snapshot.gpu.core_clock_mhz,
    'gpu.vram.used': snapshot.gpu.vram_used_mb,
    'gpu.vram.total': snapshot.gpu.vram_total_mb,
    'gpu.power': snapshot.gpu.power_draw_w,
    'gpu.fan': snapshot.gpu.fan_speed_percent,
    'cpu.usage': snapshot.cpu.total_usage_percent,
    'cpu.temp': snapshot.cpu.package_temp_c,
    'ram.usage': snapshot.ram.usage_percent,
  };
}
