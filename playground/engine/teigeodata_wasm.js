/* @ts-self-types="./teigeodata_wasm.d.ts" */

/**
 * Open packs answering place queries (D-141, D-274). Every method takes `&self`: one engine
 * answers from any number of threads at once, and adding a pack publishes a new snapshot
 * while queries go on.
 *
 * Place results are blobs with the columns `id`, `name`, `kind`, `level`, `lat`, `lon`,
 * `elevation`, `parent`, `tz`, `iso`, `population`, `population_kind` (`stated` or
 * `modelled`) and `class` (a landmark's class, such as `temple`; D-381); absent values are
 * null. A search adds `score`, and a reverse lookup adds `meters` and `bearing`.
 */
export class Engine {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        EngineFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_engine_free(ptr, 0);
    }
    /**
     * The admin units holding the point, country first.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Corrupt` if a
     * section read is corrupt.
     * @param {number} lat
     * @param {number} lon
     * @returns {Uint8Array}
     */
    adminAt(lat, lon) {
        const ret = wasm.engine_adminAt(this.__wbg_ptr, lat, lon);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The place's ancestors, parent first up to the country; no rows for a root or an
     * unknown ID.
     *
     * # Errors
     *
     * As `parent`.
     * @param {string} id
     * @returns {Uint8Array}
     */
    ancestors(id) {
        const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_ancestors(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * The outline of the place with this ID at level of detail `lod`, as one row: `id`,
     * `lod` (the level read: 0 from a pack stored at full resolution only), `tolerance_m`
     * (how far it may stray from the full-resolution outline), `vertices` and `geojson`, an
     * RFC 7946 Feature ready for a map: a `Polygon` or `MultiPolygon` (outer rings
     * counterclockwise, holes clockwise), or the place's `Point` when it has no outline.
     * Level 0 is full resolution and 1 to `COARSEST_LOD` coarser; neighbours share their
     * borders at every level. A whole country at level 0 can be megabytes: ask a coarser
     * level to draw it small. No row for an unknown ID.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `id` is not a place ID or `lod` exceeds
     * `COARSEST_LOD`; `ErrorCode::MissingComponent` without `boundaries`;
     * `ErrorCode::Corrupt` if a section read is corrupt.
     * @param {string} id
     * @param {number} lod
     * @returns {Uint8Array}
     */
    boundary(id, lod) {
        const val2 = +lod;
        const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_boundary(this.__wbg_ptr, ptr0, len0, val2);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v3;
    }
    /**
     * The place whose ISO 3166 code is `code` (`NP`, `NP-P3`, either case), or no row.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `code` is not an ISO 3166 code;
     * `ErrorCode::Corrupt` if a section read is corrupt.
     * @param {string} code
     * @returns {Uint8Array}
     */
    byIso(code) {
        const ptr0 = passStringToWasm0(code, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_byIso(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * The place's children, by ID; no rows for a leaf or an unknown ID.
     *
     * # Errors
     *
     * As `parent`.
     * @param {string} id
     * @returns {Uint8Array}
     */
    children(id) {
        const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_children(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * The great-circle distance between the places with these IDs, as one row: `meters` on
     * the mean Earth sphere and `bearing`, the initial bearing from `from` to `to` in degrees
     * clockwise from north. No row when either ID is unknown. From a point, use `distance_m`
     * and `bearing_deg` with the place's `lat` and `lon`.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if an ID is not a place ID; `ErrorCode::Corrupt` if a
     * section read is corrupt.
     * @param {string} from
     * @param {string} to
     * @returns {Uint8Array}
     */
    distance(from, to) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(from, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(to, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.engine_distance(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v3;
    }
    /**
     * The terrain height at the point, as one row: `meters` above the datum (bilinear between
     * the DEM samples around it), `source` (the DEM product's key, null where the pack does
     * not state it), `resolution_m` (the sample spacing north to south) and `datum`
     * (`EGM2008`). No row where no pack has terrain there (the sea, outside every grid).
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Corrupt` if a
     * grid read is corrupt.
     * @param {number} lat
     * @param {number} lon
     * @returns {Uint8Array}
     */
    elevationAt(lat, lon) {
        const ret = wasm.engine_elevationAt(this.__wbg_ptr, lat, lon);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The place with this ID (`tgd:np/1`), or no row; a retired ID gives its survivor.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `id` is not a place ID; `ErrorCode::Corrupt` if a
     * section read is corrupt.
     * @param {string} id
     * @returns {Uint8Array}
     */
    get(id) {
        const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_get(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * The number of packs.
     * @returns {number}
     */
    layers() {
        const ret = wasm.engine_layers(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The places nearest the place with this ID, not counting it, nearest first, at most
     * `limit`, with `meters` and `bearing` (from each place to it); no rows for an unknown ID.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `id` is not a place ID or `limit` exceeds
     * `LIMIT_MAX`; `ErrorCode::Corrupt` if a section read is corrupt.
     * @param {string} id
     * @param {number} limit
     * @returns {Uint8Array}
     */
    nearby(id, limit) {
        const val2 = +limit;
        const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_nearby(this.__wbg_ptr, ptr0, len0, val2);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v3;
    }
    /**
     * An engine with no packs.
     */
    constructor() {
        const ret = wasm.engine_new();
        this.__wbg_ptr = ret;
        EngineFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Adds a pack from its bytes (copied) with these options. Returns its layer ID, as
     * `open_file`.
     *
     * # Errors
     *
     * As `open_bytes`, and `ErrorCode::InvalidArgument` for an outline
     * level above 255.
     * @param {Uint8Array | number[]} bytes
     * @param {OpenOptions} [options]
     * @returns {number}
     */
    openBytesWith(bytes, options) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_openBytesWith(this.__wbg_ptr, ptr0, len0, isLikeNone(options) ? 0 : addToExternrefTable0(options));
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Adds a pack from its bytes (copied). Returns its layer ID, as
     * `open_file`.
     *
     * # Errors
     *
     * `ErrorCode::Corrupt` if the bytes are not a valid pack.
     * @param {Uint8Array | number[]} bytes
     * @returns {number}
     */
    openBytes(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_openBytes(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Adds a pack the browser store installed, as its journal `record` says (`set`, `parts`,
     * `pack_len`), from its head and the sections [`installedSections`] named, read from its
     * file: lazy by section, the rest never enters memory.
     *
     * # Errors
     *
     * As `openSections`; `InvalidArgument` for a record that is not a journal record.
     * @param {Uint8Array | number[]} head
     * @param {JournalRecord} record
     * @param {Float64Array} offsets
     * @param {Uint8Array[]} sections
     * @param {OpenOptions} [options]
     * @returns {number}
     */
    openInstalled(head, record, offsets, sections, options) {
        const ptr0 = passArray8ToWasm0(head, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_openInstalled(this.__wbg_ptr, ptr0, len0, record, offsets, sections, isLikeNone(options) ? 0 : addToExternrefTable0(options));
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Adds a pack from its head and some of its sections, each in its own buffer: `head` is
     * the file's first bytes, at least its head frame; `fileLen` the file's length; `offsets`
     * each section's offset in the file and `sections` its bytes, in the same order. Set
     * `options.components` (and `languages`) to what the sections hold. Returns its layer ID.
     *
     * # Errors
     *
     * `Corrupt` if the head is not a valid pack head or a section is not one of its sections
     * (or, verified eagerly, does not match its hash); `InvalidArgument` for mismatched lists.
     * @param {Uint8Array | number[]} head
     * @param {number} fileLen
     * @param {Float64Array} offsets
     * @param {Uint8Array[]} sections
     * @param {OpenOptions} [options]
     * @returns {number}
     */
    openSections(head, fileLen, offsets, sections, options) {
        const val2 = +fileLen;
        const ptr0 = passArray8ToWasm0(head, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_openSections(this.__wbg_ptr, ptr0, len0, val2, offsets, sections, isLikeNone(options) ? 0 : addToExternrefTable0(options));
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * The place's parent as the packs answer for it (one row), or no row for a root or an
     * unknown ID.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `id` is not a place ID; `ErrorCode::Corrupt` if a
     * section read is corrupt.
     * @param {string} id
     * @returns {Uint8Array}
     */
    parent(id) {
        const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_parent(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * The postal codes of the point: those of the deepest admin unit holding it that an
     * admin-aligned scheme gives codes to, with its parts (a ward's code; the point is in one
     * of them). No rows where no unit holding the point has such codes.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Corrupt` if a
     * section read is corrupt.
     * @param {number} lat
     * @param {number} lon
     * @returns {Uint8Array}
     */
    postalAt(lat, lon) {
        const ret = wasm.engine_postalAt(this.__wbg_ptr, lat, lon);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The postal codes of the place with this ID; no rows for an unknown ID.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `id` is not a place ID; `ErrorCode::Corrupt` if a
     * section read is corrupt.
     * @param {string} id
     * @returns {Uint8Array}
     */
    postalCodesOf(id) {
        const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_postalCodesOf(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * The places postal code `code` names, one row per code and place, with the code's
     * `postal`, `scheme`, `code_kind`, `valid_from` and `valid_to` columns. `scheme` names
     * one scheme (`np`, `gb`); empty means every scheme whose rule accepts the input. The
     * code may be typed with or without separators, in any case, or in another script's
     * digits. Retired codes are returned too, with their validity.
     *
     * # Errors
     *
     * `ErrorCode::Corrupt` if a section read is corrupt.
     * @param {string} code
     * @param {string} scheme
     * @returns {Uint8Array}
     */
    postalLookup(code, scheme) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(code, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(scheme, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.engine_postalLookup(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v3;
    }
    /**
     * Up to `limit` postal codes starting with `prefix`, with their places (autocomplete),
     * in code order per scheme; `scheme` as in `postal_lookup`.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `limit` exceeds `LIMIT_MAX`;
     * `ErrorCode::Corrupt` if a section read is corrupt.
     * @param {string} prefix
     * @param {string} scheme
     * @param {number} limit
     * @returns {Uint8Array}
     */
    postalPrefix(prefix, scheme, limit) {
        const val3 = +limit;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(prefix, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(scheme, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.engine_postalPrefix(this.__wbg_ptr, ptr0, len0, ptr1, len1, val3);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v4 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v4;
    }
    /**
     * Removes the pack with this layer ID; false if no pack has it. Queries already running
     * finish on the snapshot they started with; the pack's memory is released after them.
     * @param {number} layer
     * @returns {boolean}
     */
    remove(layer) {
        const ret = wasm.engine_remove(this.__wbg_ptr, layer);
        return ret !== 0;
    }
    /**
     * The places nearest the point, nearest first, at most `limit`, with `meters` and
     * `bearing` (degrees clockwise from north, from each place to the point).
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` for a point off the globe or a `limit` above
     * `LIMIT_MAX`; `ErrorCode::Corrupt` if a section read is corrupt.
     * @param {number} lat
     * @param {number} lon
     * @param {number} limit
     * @returns {Uint8Array}
     */
    reverse(lat, lon, limit) {
        const ret = wasm.engine_reverse(this.__wbg_ptr, lat, lon, limit);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * `search` ranked by `options`: near a focus point, or in strict match
     * order.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `limit` exceeds `LIMIT_MAX`, the focus is off the
     * globe or has one coordinate only, or its scale is not positive;
     * `ErrorCode::Corrupt` if a section read is corrupt.
     * @param {string} query
     * @param {number} limit
     * @param {SearchOptions} [options]
     * @returns {Uint8Array}
     */
    searchWith(query, limit, options) {
        const val2 = +limit;
        const ptr0 = passStringToWasm0(query, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_searchWith(this.__wbg_ptr, ptr0, len0, val2, isLikeNone(options) ? 0 : addToExternrefTable0(options));
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v3;
    }
    /**
     * Places matching `query`, best first, at most `limit`; "Name, Parent" ranks a named
     * parent first. The `score` column compares only within one query.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` if `limit` exceeds `LIMIT_MAX`;
     * `ErrorCode::Corrupt` if a section read is corrupt.
     * @param {string} query
     * @param {number} limit
     * @returns {Uint8Array}
     */
    search(query, limit) {
        const val2 = +limit;
        const ptr0 = passStringToWasm0(query, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_search(this.__wbg_ptr, ptr0, len0, val2);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v3;
    }
    /**
     * The time-zone area holding the point (one row; its `tz` column is the IANA zone), or no
     * row where no pack answers.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Corrupt` if a
     * section read is corrupt.
     * @param {number} lat
     * @param {number} lon
     * @returns {Uint8Array}
     */
    tzAt(lat, lon) {
        const ret = wasm.engine_tzAt(this.__wbg_ptr, lat, lon);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The offsets a civil time may have in `zone`: one row with `kind` (`unique`, `gap` for
     * a time a transition skips, `overlap` for one that happens twice), `before` and `after`
     * in seconds east of UTC (the offsets before and after the transition: for an overlap
     * the earlier and the later instant's; for a unique time both the same); no row where no
     * layer holds the zone.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` for a date that is not one of the years ±9,999 or a time
     * out of range; otherwise as `Engine::tz_offset_at`.
     * @param {string} zone
     * @param {number} year
     * @param {number} month
     * @param {number} day
     * @param {number} hour
     * @param {number} minute
     * @param {number} second
     * @returns {Uint8Array}
     */
    tzLocal(zone, year, month, day, hour, minute, second) {
        const val2 = +year;
        const val3 = +month;
        const val4 = +day;
        const val5 = +hour;
        const val6 = +minute;
        const val7 = +second;
        const ptr0 = passStringToWasm0(zone, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_tzLocal(this.__wbg_ptr, ptr0, len0, val2, val3, val4, val5, val6, val7);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v8 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v8;
    }
    /**
     * The UTC offset in force in `zone` (a tzdb name or link, `Asia/Kathmandu`) at
     * `unix_seconds`, from the first layer whose zone rules hold the zone: one row with
     * `seconds` east of UTC, `dst` (1 when tzdb counts it as daylight saving time) and
     * `abbr` (`+0545`, `EDT`); no row where no layer holds the zone.
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` for an instant outside the years ±9,999;
     * `ErrorCode::MissingComponent` if the rules that would hold the zone were not
     * installed; `ErrorCode::Corrupt` if they are corrupt.
     * @param {string} zone
     * @param {number} unixSeconds
     * @returns {Uint8Array}
     */
    tzOffsetAt(zone, unixSeconds) {
        const val2 = +unixSeconds;
        const ptr0 = passStringToWasm0(zone, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_tzOffsetAt(this.__wbg_ptr, ptr0, len0, val2);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v3;
    }
    /**
     * The earliest instant `zone`'s rules cover, in Unix seconds: one `unix` row (its first
     * transition, before which tzdb knows only local mean time; the year −9,999 for a zone
     * that never changed); no row where no layer holds the zone.
     *
     * # Errors
     *
     * As `Engine::tz_offset_at`.
     * @param {string} zone
     * @returns {Uint8Array}
     */
    tzRulesStart(zone) {
        const ptr0 = passStringToWasm0(zone, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_tzRulesStart(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * The tzdb release (`2026f`) of the first layer with zone rules; empty without rules.
     * @returns {string}
     */
    tzVersion() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_tzVersion(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Every zone name (tzdb names and links) the layers' zone rules hold, in byte order:
     * one `zone` column.
     *
     * # Errors
     *
     * `ErrorCode::MissingComponent` if no installed layer has rules and one has rules that
     * were not installed.
     * @returns {Uint8Array}
     */
    tzZones() {
        const ret = wasm.engine_tzZones(this.__wbg_ptr);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The places within `radius_m` metres of the point (great-circle), nearest first, at most `limit`,
     * with `meters` and `bearing` (from each place to the point).
     *
     * # Errors
     *
     * `ErrorCode::InvalidArgument` for a point off the globe, a negative or non-finite
     * radius, or a `limit` above `LIMIT_MAX`; `ErrorCode::Corrupt` if a section read is
     * corrupt.
     * @param {number} lat
     * @param {number} lon
     * @param {number} radiusM
     * @param {number} limit
     * @returns {Uint8Array}
     */
    withinRadius(lat, lon, radiusM, limit) {
        const ret = wasm.engine_withinRadius(this.__wbg_ptr, lat, lon, radiusM, limit);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
}
if (Symbol.dispose) Engine.prototype[Symbol.dispose] = Engine.prototype.free;

/**
 * The initial great-circle bearing from the first point to the second, in degrees clockwise
 * from true north in `[0, 360)`; 0 for one point twice or from a pole. The same digits on
 * every platform.
 *
 * # Errors
 *
 * `ErrorCode::InvalidArgument` if a latitude is outside -90..=90, a longitude outside
 * -180..=180, or any value is not finite.
 * @param {number} lat1
 * @param {number} lon1
 * @param {number} lat2
 * @param {number} lon2
 * @returns {number}
 */
export function bearingDeg(lat1, lon1, lat2, lon2) {
    const ret = wasm.bearingDeg(lat1, lon1, lat2, lon2);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * The components this build answers, in table order, comma-separated (`point, elevation,
 * admin`). A build made without one answers its queries as a layer installed without it
 * does: no rows, or `ErrorCode::MissingComponent`.
 * @returns {string}
 */
export function components() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.components();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Great-circle distance in metres between two points in degrees, by the haversine formula on
 * the mean Earth sphere (error up to about 0.5% against the ellipsoid), as every query
 * measures it, with the same digits on every platform.
 *
 * # Errors
 *
 * `ErrorCode::InvalidArgument` if a latitude is outside -90..=90, a longitude outside
 * -180..=180, or any value is not finite.
 * @param {number} lat1
 * @param {number} lon1
 * @param {number} lat2
 * @param {number} lon2
 * @returns {number}
 */
export function distanceM(lat1, lon1, lat2, lon2) {
    const ret = wasm.distanceM(lat1, lon1, lat2, lon2);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0];
}

/**
 * The search fold of a name: NFKD, lower case, optional accents dropped by script, spaces
 * collapsed, recomposed to NFC. The same fold the forge and the search index use.
 *
 * # Errors
 *
 * `ErrorCode::MissingComponent` in a build without `search`, whose Unicode tables it uses
 * (D-292).
 * @param {string} text
 * @returns {string}
 */
export function fold(text) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.fold(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * The sections a pack the browser store installed holds, as its journal `record` says, from
 * a `prefix` of its file: `[offset, length, offset, length, ...]`, or `undefined` when the
 * prefix does not hold the whole head yet (read twice as much).
 *
 * # Errors
 *
 * `Corrupt` if the prefix is not a pack head; `InvalidArgument` for a bad record.
 * @param {Uint8Array | number[]} prefix
 * @param {JournalRecord} record
 * @returns {Float64Array | undefined}
 */
export function installedSections(prefix, record) {
    const ptr0 = passArray8ToWasm0(prefix, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.installedSections(ptr0, len0, record);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * Does nothing. Measures the bare cost of a call across each boundary.
 */
export function noop() {
    wasm.noop();
}

/**
 * Calls `on_step` once per step with the step number, from 1 to `steps`, on the calling
 * thread, and returns the number of steps run. Exercises callbacks in every binding.
 *
 * # Errors
 *
 * `ErrorCode::Cancelled` if `on_step` returns `false`.
 * @param {number} steps
 * @param {(a0: number) => boolean} onStep
 * @returns {number}
 */
export function progress(steps, onStep) {
    const ret = wasm.progress(steps, onStep);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return ret[0] >>> 0;
}

/**
 * Builds `rows` deterministic sample places as a blob with the columns `id` (`u32`), `lat`
 * and `lon` (`f64`, degrees), `elevation` (`f32`, metres) and `name` (UTF-8). Exercises the
 * result path of every binding.
 *
 * # Errors
 *
 * `ErrorCode::InvalidArgument` if `rows` exceeds `SAMPLE_ROWS_MAX`.
 * @param {number} rows
 * @returns {Uint8Array}
 */
export function sampleRows(rows) {
    const ret = wasm.sampleRows(rows);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v1;
}

/**
 * The library version, from its package metadata.
 * @returns {string}
 */
export function version() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.version();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_boolean_get_4f1baf9a00be3c74: function(arg0) {
            const v = arg0;
            const ret = typeof(v) === 'boolean' ? v : undefined;
            return isLikeNone(ret) ? 0xFFFFFF : ret ? 1 : 0;
        },
        __wbg___wbindgen_is_falsy_d864b623500ae65d: function(arg0) {
            const ret = !arg0;
            return ret;
        },
        __wbg___wbindgen_is_null_c22311b1cc228e3a: function(arg0) {
            const ret = arg0 === null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_be5b34249ff1fc15: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_number_get_934b640d0889af78: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64((arg0 >>> 0) + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_7ce8eee7fd327545: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_4c88d8bf264fd984: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_call_a410eb4b59ed5965: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_from_eac91875c8a5ffa4: function(arg0) {
            const ret = Array.from(arg0);
            return ret;
        },
        __wbg_get_2080c49015e35254: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_unchecked_e6325ad374eff861: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_instanceof_Uint8Array_d7aeb0dcb3108fea: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Uint8Array;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_length_186163212ffe79c3: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_5a8bb8abe1e54e15: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_b03a0fabb109b1e9: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_new_787b9fb330d55d78: function(arg0, arg1) {
            const ret = new Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_new_from_slice_aaf85a9746711968: function(arg0, arg1) {
            const ret = new Float64Array(getArrayF64FromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_prototypesetcall_13162965d1fe7aa3: function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
        },
        __wbg_prototypesetcall_a366c038a3918afa: function(arg0, arg1, arg2) {
            Float64Array.prototype.set.call(getArrayF64FromWasm0(arg0, arg1), arg2);
        },
        __wbg_set_2793479888329cb6: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbindgen_generic_0000000000000001: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            table.set(0, undefined);
            table.set(1024, undefined);
            table.set(1025, null);
            table.set(1026, true);
            table.set(1027, false);
        },
    };
    return {
        __proto__: null,
        "./teigeodata_wasm_bg.js": import0,
    };
}

const EngineFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_engine_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function getArrayF64FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat64ArrayMemory0().subarray(ptr / 8, ptr / 8 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat64ArrayMemory0 = null;
function getFloat64ArrayMemory0() {
    if (cachedFloat64ArrayMemory0 === null || cachedFloat64ArrayMemory0.byteLength === 0) {
        cachedFloat64ArrayMemory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(bytes) {
    numBytesDecoded += bytes.length;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true });
        cachedTextDecoder.decode();
        numBytesDecoded = bytes.length;
    }
    return cachedTextDecoder.decode(bytes);
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat64ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

let __wbg_init_promise;
async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;
    if (__wbg_init_promise === undefined) {
        __wbg_init_promise = __wbg_init_once(module_or_path);
        __wbg_init_promise.catch(() => { __wbg_init_promise = undefined; });
    }
    return __wbg_init_promise;
}

async function __wbg_init_once(module_or_path) {

    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('teigeodata_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
