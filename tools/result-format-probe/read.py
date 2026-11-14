import json, struct, sys, time
blob = open(sys.argv[1], "rb").read(); js = open(sys.argv[2]).read()
def cols(b):
    m = memoryview(b); rows, n = struct.unpack_from("<II", b, 4); out = {}
    for i in range(n):
        d = 16 + 40 * i; name = bytes(b[d:d+16]).rstrip(b"\0").decode()
        t, at, ln, sat, _ = struct.unpack_from("<5I", b, d + 16)
        if t == 7: out[name] = (m[at:at + 4 * (rows + 1)].cast("I"), sat)
        else: out[name] = m[at:at + ln].cast({1: "i", 2: "I", 6: "d"}[t])
    return rows, out
def read_blob():
    rows, c = cols(blob); co = sum(c["lat"]) + sum(c["lon"]); el = nu = by = 0
    for e in c["elevation"]:
        if e == -2147483648: nu += 1
        else: el += e
    for k in ("id", "name", "parent", "tz", "iso"):
        off, sat = c[k]; o0 = off[0]; base = sat - o0
        for i in range(rows):
            s = blob[base + off[i]:base + off[i + 1]].decode(); by += len(s)
            if not s and k not in ("id", "name"): nu += 1
    return co, el, nu, by
def read_json():
    co = el = nu = by = 0
    for r in json.loads(js):
        co += r["lat"] + r["lon"]
        if r["elevation"] is None: nu += 1
        else: el += r["elevation"]
        for k in ("id", "name", "parent", "timezone", "iso_code"):
            s = r.get(k)
            if s is None:
                if k not in ("id", "name"): nu += 1
            else: by += len(s)
    return co, el, nu, by
def med(f):
    t = []
    for _ in range(21):
        s = time.perf_counter(); f(); t.append((time.perf_counter() - s) * 1e6)
    return sorted(t)[10]
print(read_blob(), read_json())
print(f"python {sys.version.split()[0]}: blob read-all {med(read_blob):.0f} µs; numbers only {med(lambda: (lambda r: sum(r[1]['lat']) + sum(r[1]['lon']) + sum(r[1]['elevation']))(cols(blob))):.0f} µs; json.loads+read {med(read_json):.0f} µs")
