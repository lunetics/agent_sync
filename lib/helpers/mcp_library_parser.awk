# Strict, bounded JSON parser for the read-only MCP library schema v1.
# Invoked with LC_ALL=C so JSON strings are scanned byte-for-byte and escaped
# Unicode keys can be compared with literal UTF-8 keys without locale surprises.

function fail(message) {
    if (error == "") error = message
    return 0
}

function init_bytes(    i) {
    for (i = 0; i < 256; i++) byte_value[sprintf("%c", i)] = i
}

function byte_at(at) { return byte_value[substr(json, at, 1)] }
function hex_byte(value) { return sprintf("%02x", value) }

function utf8(codepoint,    output) {
    if (codepoint <= 127) return sprintf("%c", codepoint)
    if (codepoint <= 2047) return sprintf("%c%c", 192 + int(codepoint / 64), 128 + (codepoint % 64))
    if (codepoint <= 65535) return sprintf("%c%c%c", 224 + int(codepoint / 4096), 128 + (int(codepoint / 64) % 64), 128 + (codepoint % 64))
    return sprintf("%c%c%c%c", 240 + int(codepoint / 262144), 128 + (int(codepoint / 4096) % 64), 128 + (int(codepoint / 64) % 64), 128 + (codepoint % 64))
}

function bytes_hex(value,    i, output) {
    output = ""
    for (i = 1; i <= length(value); i++) output = output hex_byte(byte_value[substr(value, i, 1)])
    return output
}

function display_text(value,    i, byte, output) {
    output = ""
    for (i = 1; i <= length(value); i++) {
        byte = byte_value[substr(value, i, 1)]
        if (byte == 9) output = output "\\t"
        else if (byte == 10) output = output "\\n"
        else if (byte == 13) output = output "\\r"
        else if (byte < 32 || byte == 127) output = output sprintf("\\u%04x", byte)
        else if (byte == 194 && i < length(value) && byte_value[substr(value, i + 1, 1)] >= 128 && byte_value[substr(value, i + 1, 1)] <= 159) {
            i++
            output = output sprintf("\\u00%02x", byte_value[substr(value, i, 1)])
        }
        else output = output substr(value, i, 1)
    }
    return output
}

function hex_value(value,    i, digit, output) {
    output = 0
    for (i = 1; i <= length(value); i++) {
        digit = index("0123456789abcdef", tolower(substr(value, i, 1))) - 1
        output = output * 16 + digit
    }
    return output
}

function skip_space() {
    while (position <= length(json) && substr(json, position, 1) ~ /[ \t\r\n]/) position++
}

function append_codepoint(codepoint, raw_hex,    value) {
    if (codepoint >= 55296 && codepoint <= 57343) {
        return fail("unpaired Unicode surrogate escape")
    }
    value = utf8(codepoint)
    string_value = string_value value
    string_canonical = string_canonical bytes_hex(value)
    return 1
}

function append_literal_utf8(    first, second, third, fourth, bytes) {
    first = byte_at(position)
    if (first < 128) {
        string_value = string_value substr(json, position, 1)
        string_canonical = string_canonical hex_byte(first)
        position++
        return 1
    }
    if (first >= 194 && first <= 223) {
        second = byte_at(position + 1)
        if (second < 128 || second > 191) return fail("invalid UTF-8 in JSON string")
        bytes = substr(json, position, 2)
        position += 2
    } else if (first == 224) {
        second = byte_at(position + 1); third = byte_at(position + 2)
        if (second < 160 || second > 191 || third < 128 || third > 191) return fail("invalid UTF-8 in JSON string")
        bytes = substr(json, position, 3)
        position += 3
    } else if (first >= 225 && first <= 236) {
        second = byte_at(position + 1); third = byte_at(position + 2)
        if (second < 128 || second > 191 || third < 128 || third > 191) return fail("invalid UTF-8 in JSON string")
        bytes = substr(json, position, 3)
        position += 3
    } else if (first == 237) {
        second = byte_at(position + 1); third = byte_at(position + 2)
        if (second < 128 || second > 159 || third < 128 || third > 191) return fail("invalid UTF-8 in JSON string")
        bytes = substr(json, position, 3)
        position += 3
    } else if (first >= 238 && first <= 239) {
        second = byte_at(position + 1); third = byte_at(position + 2)
        if (second < 128 || second > 191 || third < 128 || third > 191) return fail("invalid UTF-8 in JSON string")
        bytes = substr(json, position, 3)
        position += 3
    } else if (first == 240) {
        second = byte_at(position + 1); third = byte_at(position + 2); fourth = byte_at(position + 3)
        if (second < 144 || second > 191 || third < 128 || third > 191 || fourth < 128 || fourth > 191) return fail("invalid UTF-8 in JSON string")
        bytes = substr(json, position, 4)
        position += 4
    } else if (first >= 241 && first <= 243) {
        second = byte_at(position + 1); third = byte_at(position + 2); fourth = byte_at(position + 3)
        if (second < 128 || second > 191 || third < 128 || third > 191 || fourth < 128 || fourth > 191) return fail("invalid UTF-8 in JSON string")
        bytes = substr(json, position, 4)
        position += 4
    } else if (first == 244) {
        second = byte_at(position + 1); third = byte_at(position + 2); fourth = byte_at(position + 3)
        if (second < 128 || second > 143 || third < 128 || third > 191 || fourth < 128 || fourth > 191) return fail("invalid UTF-8 in JSON string")
        bytes = substr(json, position, 4)
        position += 4
    } else return fail("invalid UTF-8 in JSON string")
    string_value = string_value bytes
    string_canonical = string_canonical bytes_hex(bytes)
    return 1
}

function parse_string(    escape, hex, codepoint, low_hex, low) {
    if (substr(json, position, 1) != "\"") return fail("expected JSON string")
    position++
    string_value = ""
    string_canonical = ""
    while (position <= length(json)) {
        if (substr(json, position, 1) == "\"") {
            position++
            return 1
        }
        if (substr(json, position, 1) == "\\") {
            position++
            if (position > length(json)) return fail("unterminated JSON escape")
            escape = substr(json, position, 1)
            if (escape == "u") {
                hex = substr(json, position + 1, 4)
                if (length(hex) != 4 || hex !~ /^[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]$/) return fail("invalid Unicode escape")
                codepoint = hex_value(hex)
                position += 5
                if (codepoint >= 55296 && codepoint <= 56319 && substr(json, position, 2) == "\\u") {
                    low_hex = substr(json, position + 2, 4)
                    if (low_hex ~ /^[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]$/) {
                        low = hex_value(low_hex)
                        if (low >= 56320 && low <= 57343) {
                            codepoint = 65536 + (codepoint - 55296) * 1024 + low - 56320
                            position += 6
                        }
                    }
                }
                if (!append_codepoint(codepoint, hex)) return 0
                continue
            }
            if (escape == "\"" || escape == "\\" || escape == "/") {
                string_value = string_value escape
                string_canonical = string_canonical hex_byte(byte_value[escape])
            } else if (escape == "b") {
                string_value = string_value sprintf("%c", 8); string_canonical = string_canonical "08"
            } else if (escape == "f") {
                string_value = string_value sprintf("%c", 12); string_canonical = string_canonical "0c"
            } else if (escape == "n") {
                string_value = string_value sprintf("%c", 10); string_canonical = string_canonical "0a"
            } else if (escape == "r") {
                string_value = string_value sprintf("%c", 13); string_canonical = string_canonical "0d"
            } else if (escape == "t") {
                string_value = string_value sprintf("%c", 9); string_canonical = string_canonical "09"
            } else return fail("invalid JSON escape")
            position++
            continue
        }
        if (byte_at(position) < 32) return fail("control character in JSON string")
        if (!append_literal_utf8()) return 0
    }
    return fail("unterminated JSON string")
}

function new_node(kind, raw,    node) {
    node = ++node_count
    node_kind[node] = kind
    node_raw[node] = raw
    return node
}

function parse_array(depth,    node, item, char) {
    node = new_node("array", "")
    position++
    skip_space()
    if (substr(json, position, 1) == "]") { position++; last_node = node; return 1 }
    while (position <= length(json)) {
        if (!parse_value(depth + 1)) return 0
        item = last_node
        array_count[node]++
        array_item[node SUBSEP array_count[node]] = item
        skip_space()
        char = substr(json, position, 1)
        if (char == "]") { position++; last_node = node; return 1 }
        if (char != ",") return fail("expected comma in JSON array")
        position++
        skip_space()
    }
    return fail("unterminated JSON array")
}

function parse_object(depth,    node, key, canonical, value, char) {
    node = new_node("object", "")
    position++
    skip_space()
    if (substr(json, position, 1) == "}") { position++; last_node = node; return 1 }
    while (position <= length(json)) {
        if (!parse_string()) return 0
        key = string_value
        canonical = string_canonical
        if ((node SUBSEP canonical) in object_seen) return fail("duplicate JSON key '" display_text(key) "'")
        object_seen[node SUBSEP canonical] = 1
        skip_space()
        if (substr(json, position, 1) != ":") return fail("expected colon after JSON object key")
        position++
        if (!parse_value(depth + 1)) return 0
        value = last_node
        object_count[node]++
        object_key[node SUBSEP object_count[node]] = key
        object_value[node SUBSEP object_count[node]] = value
        skip_space()
        char = substr(json, position, 1)
        if (char == "}") { position++; last_node = node; return 1 }
        if (char != ",") return fail("expected comma in JSON object")
        position++
        skip_space()
    }
    return fail("unterminated JSON object")
}

function parse_value(depth,    char, rest, raw, node) {
    if (depth > max_depth) return fail("JSON nesting exceeds depth limit " max_depth)
    skip_space()
    char = substr(json, position, 1)
    if (char == "\"") {
        if (!parse_string()) return 0
        node = new_node("string", string_value)
        node_canonical[node] = string_canonical
        last_node = node
        return 1
    }
    if (char == "{") return parse_object(depth)
    if (char == "[") return parse_array(depth)
    rest = substr(json, position)
    if (substr(rest, 1, 4) == "true" && substr(rest, 5, 1) !~ /[[:alnum:]_]/) { position += 4; last_node = new_node("boolean", "true"); return 1 }
    if (substr(rest, 1, 5) == "false" && substr(rest, 6, 1) !~ /[[:alnum:]_]/) { position += 5; last_node = new_node("boolean", "false"); return 1 }
    if (substr(rest, 1, 4) == "null" && substr(rest, 5, 1) !~ /[[:alnum:]_]/) { position += 4; last_node = new_node("null", "null"); return 1 }
    if (match(rest, /^-?(0|[1-9][0-9]*)([.][0-9]+)?([eE][+-]?[0-9]+)?/)) {
        raw = substr(rest, 1, RLENGTH)
        position += RLENGTH
        last_node = new_node("number", raw)
        return 1
    }
    return fail("invalid JSON value")
}

function object_field(object, wanted,    i) {
    for (i = 1; i <= object_count[object]; i++) if (object_key[object SUBSEP i] == wanted) return object_value[object SUBSEP i]
    return 0
}

function require_field(object, name, label,    value) {
    value = object_field(object, name)
    if (value == 0) fail(label " requires field '" name "'")
    return value
}

function allow_only(object, allowed, label,    i, key) {
    for (i = 1; i <= object_count[object]; i++) {
        key = object_key[object SUBSEP i]
        if (!(key in allowed)) return fail("unknown " label " field '" display_text(key) "'")
    }
    return 1
}

function require_kind(node, kind, label) {
    if (node_kind[node] != kind) return fail(label " must be " kind)
    return 1
}

function validate_string_array(node, label,    i, item) {
    if (!require_kind(node, "array", label)) return 0
    for (i = 1; i <= array_count[node]; i++) {
        item = array_item[node SUBSEP i]
        if (node_kind[item] != "string") return fail(label " must contain only strings")
    }
    return 1
}

function validate_provenance(node,    allowed, i, value) {
    if (!require_kind(node, "object", "provenance")) return 0
    allowed["homepage"] = allowed["repository"] = allowed["artifact"] = 1
    if (!allow_only(node, allowed, "provenance")) return 0
    for (i = 1; i <= object_count[node]; i++) {
        value = object_value[node SUBSEP i]
        if (node_kind[value] != "string" && node_kind[value] != "null") return fail("provenance values must be strings or null")
    }
    return 1
}

function validate_extensions(node,    i, key) {
    if (!require_kind(node, "object", "extensions")) return 0
    for (i = 1; i <= object_count[node]; i++) {
        key = object_key[node SUBSEP i]
        if (key !~ /^[a-z0-9][a-z0-9-]*\.[a-z0-9][a-z0-9.-]*$/) return fail("extension key must be namespaced: '" display_text(key) "'")
    }
    return 1
}

function validate_connection(node,    allowed, type, command, args, url) {
    if (!require_kind(node, "object", "connection")) return 0
    allowed["type"] = allowed["command"] = allowed["args"] = allowed["url"] = 1
    if (!allow_only(node, allowed, "connection")) return 0
    type = require_field(node, "type", "connection")
    if (error != "") return 0
    if (!require_kind(type, "string", "connection.type")) return 0
    if (node_raw[type] == "stdio") {
        command = require_field(node, "command", "stdio connection")
        args = require_field(node, "args", "stdio connection")
        if (error != "") return 0
        if (!require_kind(command, "string", "connection.command") || node_raw[command] == "") return fail("connection.command must be a non-empty string")
        if (!validate_string_array(args, "connection.args")) return 0
        if (object_field(node, "url") != 0) return fail("stdio connection must not define url")
    } else if (node_raw[type] == "http") {
        url = require_field(node, "url", "http connection")
        if (error != "") return 0
        if (!require_kind(url, "string", "connection.url") || node_raw[url] == "") return fail("connection.url must be a non-empty string")
        if (object_field(node, "command") != 0 || object_field(node, "args") != 0) return fail("http connection must not define command or args")
    } else return fail("connection.type must be 'stdio' or 'http'")
    return 1
}

function validate_requirements(node,    allowed, binaries, inputs) {
    if (!require_kind(node, "object", "requirements")) return 0
    allowed["binaries"] = allowed["inputs"] = 1
    if (!allow_only(node, allowed, "requirements")) return 0
    binaries = require_field(node, "binaries", "requirements")
    inputs = require_field(node, "inputs", "requirements")
    if (error != "") return 0
    if (!validate_string_array(binaries, "requirements.binaries")) return 0
    if (!validate_string_array(inputs, "requirements.inputs")) return 0
    if (array_count[inputs] != 0) return fail("requirements.inputs is unsupported in MCP library schema v1; use an empty array")
    return 1
}

function validate_manifest(root,    allowed, schema, id, title, description, provenance, connection, requirements, extensions) {
    if (node_kind[root] != "object") return fail("manifest root must be an object")
    allowed["schema_version"] = allowed["id"] = allowed["title"] = allowed["description"] = allowed["provenance"] = allowed["connection"] = allowed["requirements"] = allowed["extensions"] = 1
    if (!allow_only(root, allowed, "top-level")) return 0
    schema = require_field(root, "schema_version", "manifest")
    id = require_field(root, "id", "manifest")
    title = require_field(root, "title", "manifest")
    connection = require_field(root, "connection", "manifest")
    requirements = require_field(root, "requirements", "manifest")
    if (error != "") return 0
    if (node_kind[schema] != "number" || node_raw[schema] != "1") return fail("unsupported schema_version '" node_raw[schema] "'")
    if (!require_kind(id, "string", "id") || node_raw[id] !~ /^[A-Za-z0-9][A-Za-z0-9_-]*$/ || length(node_raw[id]) > 64) return fail("id must match [A-Za-z0-9][A-Za-z0-9_-]{0,63}")
    if (!require_kind(title, "string", "title") || node_raw[title] == "") return fail("title must be a non-empty string")
    description = object_field(root, "description")
    if (description != 0 && !require_kind(description, "string", "description")) return 0
    provenance = object_field(root, "provenance")
    if (provenance != 0 && !validate_provenance(provenance)) return 0
    if (!validate_connection(connection) || !validate_requirements(requirements)) return 0
    extensions = object_field(root, "extensions")
    if (extensions != 0 && !validate_extensions(extensions)) return 0
    if (expected_id != "" && node_raw[id] != expected_id) return fail("manifest id '" node_raw[id] "' does not match requested id '" expected_id "'")
    manifest_id = node_raw[id]
    manifest_title = node_raw[title]
    return 1
}

BEGIN {
    init_bytes()
    read_status = 0
    read_bytes = 0
    have_line = 0
    while ((read_status = getline line < ARGV[1]) > 0) {
        # getline strips delimiters. A separator is known to exist only before
        # a later record, so do not invent one after a final unterminated line.
        if (have_line) read_bytes++
        read_bytes += length(line)
        if (read_bytes > max_bytes) {
            fail("manifest exceeds byte limit " max_bytes)
            break
        }
        if (have_line) json = json "\n"
        json = json line
        have_line = 1
    }
    close(ARGV[1])
    if (read_status < 0) fail("cannot read manifest")
    if (error != "") {
        print error > "/dev/stderr"
        exit 1
    }
    position = 1
    skip_space()
    if (!parse_value(1)) {
        print error > "/dev/stderr"
        exit 1
    }
    root = last_node
    skip_space()
    if (position <= length(json)) fail("trailing content after JSON value")
    if (error == "" && !validate_manifest(root)) { }
    if (error != "") {
        print error > "/dev/stderr"
        exit 1
    }
    if (output_mode == "metadata") printf "%s\t%s\n", manifest_id, display_text(manifest_title)
}
