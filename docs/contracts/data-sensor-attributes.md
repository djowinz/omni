# `data-sensor` Attribute Contract

**Status:** Authoritative (Phase 0). Changes require umbrella update.

Bound elements in sanitized bundle HTML bind to live sensor updates via the privileged bootstrap. Ammonia whitelists the attributes below on the allowed tag set; the bootstrap walks the DOM once at load, builds an index `Map<SensorPath, Element[]>`, then `__omni_update(values)` applies O(n) updates without re-querying.

## 1. Attributes

| Attribute | Required | Value grammar | Semantics |
|---|---|---|---|
| `data-sensor` | yes | `sensor-path` | Bind element to a sensor reading |
| `data-sensor-format` | no | `raw|percent|bytes|temperature|frequency` | How to stringify numeric value |
| `data-sensor-precision` | no | `[0-9]+` (0–6) | Decimal places for numeric formats |
| `data-sensor-threshold-warn` | no | signed decimal | If value ≥ this, element gains class `sensor-warn` |
| `data-sensor-threshold-critical` | no | signed decimal | If value ≥ this, element gains class `sensor-critical` (supersedes warn) |
| `data-sensor-target` | no | `text|attr:<name>|class|style-var:<name>` | Where the formatted value is written; default `text` |

## 2. BNF grammar

```bnf
sensor-path     ::= segment ("." segment){0,3}
segment         ::= [a-z][a-z0-9_-]{0,31}
format          ::= "raw" | "percent" | "bytes"
                  | "temperature" | "frequency"
precision       ::= integer-literal            ; 0..=6
threshold       ::= "-"? [0-9]+ ("." [0-9]+)?
target          ::= "text"
                  | "class"
                  | "attr:" attr-name
                  | "style-var:" css-ident
attr-name       ::= [a-zA-Z_][a-zA-Z0-9_-]*
css-ident       ::= [a-zA-Z_-][a-zA-Z0-9_-]*
```

Paths exceeding 4 segments, precision > 6, malformed thresholds, or unknown formats → the bootstrap logs a warning and skips the binding. The element remains in the DOM unchanged.

## 3. Sensor path vocabulary

Initial Phase 0 vocabulary (extensions require umbrella update):

```
cpu.usage            cpu.temp           cpu.freq
gpu.usage            gpu.temp           gpu.freq        gpu.vram.used   gpu.vram.total
mem.used             mem.total          mem.pct
disk.<id>.used       disk.<id>.total    disk.<id>.pct
net.<iface>.rx       net.<iface>.tx
battery.pct          battery.state
time.now             time.uptime
fps.current          fps.min            fps.max
sys.hostname
```

`<id>` and `<iface>` are host-populated identifiers matching `[a-z0-9_-]{1,32}`.

Bundles MUST list every sensor path they reference in `manifest.sensor_requirements`. Install-time, host warns if any path is not provided by the current platform.

## 4. Allowed tags

`data-sensor*` attributes pass ammonia sanitization on these tags only:

```
span   div   p     li    td    th    tr    thead  tbody   table
h1 h2 h3 h4 h5 h6  pre   code  small em    strong b       i
output meter        progress
```

On any other tag, the attribute is stripped by sanitize and a `FileReport` note is emitted.

## 5. Target semantics

| `data-sensor-target` | Effect of each update |
|---|---|
| *(unset)* or `text` | `element.textContent = formatted` |
| `attr:<name>` | `element.setAttribute("<name>", formatted)` (name whitelist same as ammonia's allowed attrs) |
| `class` | `element.className = formatted` (value must match `[a-zA-Z0-9_-\s]*`; bootstrap rejects otherwise) |
| `style-var:<ident>` | `element.style.setProperty("--<ident>", formatted)` (only CSS custom properties; no other inline style writes allowed) |

Threshold classes (`sensor-warn`, `sensor-critical`) are always applied in addition to target writes.

## 6. Formatting reference

| Format | Input | Output |
|---|---|---|
| `raw` | number | `value.toFixed(precision)` |
| `percent` | 0..1 float or 0..100 int | scales to 0..100, `.toFixed(precision) + "%"` |
| `bytes` | integer bytes | SI prefix (`KB`, `MB`, `GB`, `TB`) at given precision |
| `temperature` | °C float | `"<value.toFixed(precision)>°C"` |
| `frequency` | Hz integer | `MHz`/`GHz` prefix at given precision |

The formatter is implemented once inside the privileged bootstrap; bundles cannot override it.
