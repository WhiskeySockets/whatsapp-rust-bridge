/**
 * Generates the protobufjs-style namespace declarations the bridge ships under
 * the `./proto-types` sub-export. Reads the ts-proto output (`ts/generated/whatsapp.ts`,
 * which is itself regenerated when wacore's proto schema changes) and writes
 * `ts/proto-types.d.ts`, which becomes the type definitions consumers see when
 * they `import { proto } from 'whatsapp-rust-bridge/proto-types'`.
 *
 * Run via `bun run gen:proto-types` (see package.json). The output is committed
 * so consumers don't need to regenerate at install time.
 *
 * Why protobufjs-style and not the raw ts-proto shape: code migrating from
 * `@whiskeysockets/baileys` expects `proto.Message.fromObject({...})`,
 * `proto.Message.ProtocolMessage.Type.REVOKE`, `interface IMessage` — the same
 * surface `pbjs/pbts` historically generated. The ts-proto output uses flat
 * underscore names (`Message_VideoMessage`) and lacks the `I`-prefix interface
 * convention. This script bridges the two without forcing a separate proto
 * compilation pass.
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SRC = join(HERE, '..', 'ts', 'generated', 'whatsapp.ts')
const OUT = join(HERE, '..', 'ts', 'proto-types.d.ts')

type EnumMember = { name: string; value: number }
type EnumDef = { kind: 'enum'; flatName: string; members: EnumMember[] }
type FieldDef = { name: string; type: string; optional: boolean; repeated: boolean }
type InterfaceDef = { kind: 'interface'; flatName: string; fields: FieldDef[] }
type Def = EnumDef | InterfaceDef

// ─── Parse ───────────────────────────────────────────────────────────────

const src = readFileSync(SRC, 'utf8')

const enumDefs: EnumDef[] = []
const interfaceDefs: InterfaceDef[] = []
const enumNames = new Set<string>()

for (const m of src.matchAll(/^export enum (\w+) \{\n([\s\S]*?)^\}$/gm)) {
	const flatName = m[1]!
	const body = m[2]!
	const members: EnumMember[] = []
	for (const em of body.matchAll(/^\s*(\w+) = (-?\d+),?$/gm)) {
		const name = em[1]!
		if (name === 'UNRECOGNIZED') continue
		members.push({ name, value: Number(em[2]!) })
	}
	enumDefs.push({ kind: 'enum', flatName, members })
	enumNames.add(flatName)
}

for (const m of src.matchAll(/^export interface (\w+) \{\n([\s\S]*?)^\}$/gm)) {
	const flatName = m[1]!
	const body = m[2]!
	const fields: FieldDef[] = []
	for (const fm of body.matchAll(/^\s+(\w+)(\??):\s*([^\n;]+);$/gm)) {
		let raw = fm[3]!.trim()
		const optional = fm[2] === '?'
		raw = raw.replace(/\s*\|\s*undefined$/, '')
		let repeated = false
		if (raw.endsWith('[]')) {
			repeated = true
			raw = raw.slice(0, -2)
		}
		fields.push({ name: fm[1]!, type: raw, optional, repeated })
	}
	interfaceDefs.push({ kind: 'interface', flatName, fields })
}

const defs: Def[] = [...enumDefs, ...interfaceDefs]

// ─── Build namespace tree ────────────────────────────────────────────────

interface TreeNode {
	def?: Def
	children: Map<string, TreeNode>
}

const root: TreeNode = { children: new Map() }

const splitPath = (n: string): string[] => n.split('_')

const insertAtPath = (path: string[], def: Def): void => {
	let cursor = root
	for (let i = 0; i < path.length - 1; i++) {
		const seg = path[i]!
		let next = cursor.children.get(seg)
		if (!next) {
			next = { children: new Map() }
			cursor.children.set(seg, next)
		}
		cursor = next
	}
	const leaf = path[path.length - 1]!
	let leafNode = cursor.children.get(leaf)
	if (!leafNode) {
		leafNode = { children: new Map() }
		cursor.children.set(leaf, leafNode)
	}
	leafNode.def = def
}

for (const def of defs) {
	insertAtPath(splitPath(def.flatName), def)
}

// ─── Type translation ────────────────────────────────────────────────────

const PRIMITIVES = new Set(['string', 'number', 'boolean', 'Uint8Array', 'bigint', 'any'])

const translateType = (raw: string): string => {
	const trimmed = raw.trim()
	if (PRIMITIVES.has(trimmed)) return trimmed
	const mapMatch = trimmed.match(/^\{\s*\[\s*key:\s*string\s*\]:\s*(.+?)\s*\}$/)
	if (mapMatch) return `{ [k: string]: ${translateType(mapMatch[1]!)} }`
	if (/^[A-Z][\w_]*$/.test(trimmed)) {
		const isEnum = enumNames.has(trimmed)
		const parts = trimmed.split('_')
		if (isEnum) return 'proto.' + parts.join('.')
		const last = parts.pop()!
		parts.push('I' + last)
		return 'proto.' + parts.join('.')
	}
	return trimmed
}

const renderField = (f: FieldDef, withPublic: boolean): string => {
	const baseType = translateType(f.type)
	const prefix = withPublic ? 'public ' : ''
	// protobufjs convention: every field is optional. Even repeated arrays
	// (which ts-proto declares as `T[]` non-optional) get relaxed to optional
	// here so callers don't have to spell out empty arrays for every nested
	// builder they construct.
	if (f.repeated) {
		return `${prefix}${f.name}?: ${baseType}[];`
	}
	return `${prefix}${f.name}?: (${baseType}|null);`
}

// ─── Render ──────────────────────────────────────────────────────────────

const HEADER = `// Auto-generated by scripts/gen-protobufjs-dts.ts. Do not edit by hand.
// Source: ts/generated/whatsapp.ts (ts-proto output, regenerated when the
// wacore proto schema changes).

type Long = number;
declare namespace $protobuf {
\tinterface Writer { finish(): Uint8Array; }
\ttype Reader = Uint8Array;
\tinterface IConversionOptions { [key: string]: any; }
}

export namespace proto {
`

// Hand-curated aliases for callers that imported the upstream-Baileys
// PascalCase form of acronym-prefixed types (`Adv*`) before the bridge
// switched to preserving the proto's actual casing (`ADV*`). Keeps existing
// bot code type-checking against the new generated surface.
const HISTORICAL_TYPE_ALIASES: Array<[alias: string, target: string]> = [
	['IAdvDeviceIdentity', 'IADVDeviceIdentity'],
	['AdvDeviceIdentity', 'ADVDeviceIdentity'],
	['IAdvSignedDeviceIdentity', 'IADVSignedDeviceIdentity'],
	['AdvSignedDeviceIdentity', 'ADVSignedDeviceIdentity'],
	['IAdvSignedDeviceIdentityHmac', 'IADVSignedDeviceIdentityHMAC'],
	['AdvSignedDeviceIdentityHmac', 'ADVSignedDeviceIdentityHMAC'],
	['IAdvSignedKeyIndexList', 'IADVSignedKeyIndexList'],
	['AdvSignedKeyIndexList', 'ADVSignedKeyIndexList'],
	['IAdvKeyIndexList', 'IADVSignedKeyIndexList'],
	['AdvKeyIndexList', 'ADVSignedKeyIndexList'],
	['AdvEncryptionType', 'ADVEncryptionType']
]

const FOOTER =
	HISTORICAL_TYPE_ALIASES.map(([alias, target]) => `\texport type ${alias} = ${target};`).join('\n') +
	`\n}\n`

const indent = (n: number) => '\t'.repeat(n)

const renderEnum = (def: EnumDef, leafName: string, depth: number): string => {
	let s = `\n${indent(depth)}enum ${leafName} {\n`
	for (const m of def.members) {
		s += `${indent(depth + 1)}${m.name} = ${m.value},\n`
	}
	s += `${indent(depth)}}\n`
	return s
}

const renderInterface = (def: InterfaceDef, leafName: string, depth: number): string => {
	let s = ''
	s += `\n${indent(depth)}interface I${leafName} {\n`
	for (const f of def.fields) {
		s += `${indent(depth + 1)}${renderField(f, false)}\n`
	}
	s += `${indent(depth)}}\n`
	s += `\n${indent(depth)}class ${leafName} implements I${leafName} {\n`
	s += `${indent(depth + 1)}constructor(p?: I${leafName});\n`
	for (const f of def.fields) {
		s += `${indent(depth + 1)}${renderField(f, true)}\n`
	}
	s += `${indent(depth + 1)}public static create(p?: I${leafName}): ${leafName};\n`
	s += `${indent(depth + 1)}public static fromObject(d: { [k: string]: any }): ${leafName};\n`
	s += `${indent(depth + 1)}public static toObject(m: ${leafName}, o?: $protobuf.IConversionOptions): { [k: string]: any };\n`
	s += `${indent(depth + 1)}public static encode(m: I${leafName}, w?: $protobuf.Writer): $protobuf.Writer;\n`
	s += `${indent(depth + 1)}public static decode(r: ($protobuf.Reader|Uint8Array), l?: number): ${leafName};\n`
	s += `${indent(depth + 1)}public toJSON(): { [k: string]: any };\n`
	s += `${indent(depth)}}\n`
	return s
}

const renderTree = (node: TreeNode, depth: number): string => {
	let s = ''
	const sortedChildren = [...node.children.entries()].toSorted(([a], [b]) => a.localeCompare(b))
	for (const [name, child] of sortedChildren) {
		if (child.def?.kind === 'enum') {
			s += renderEnum(child.def, name, depth)
		} else if (child.def?.kind === 'interface') {
			s += renderInterface(child.def, name, depth)
		}
		if (child.children.size > 0) {
			s += `\n${indent(depth)}namespace ${name} {\n`
			s += renderTree(child, depth + 1)
			s += `${indent(depth)}}\n`
		}
	}
	return s
}

const out = HEADER + renderTree(root, 1) + FOOTER

writeFileSync(OUT, out)
console.log(
	`Wrote ${out.length.toLocaleString()} bytes (${out.split('\n').length} lines) to ${OUT}\n` +
		`Defs: ${enumDefs.length} enums, ${interfaceDefs.length} interfaces.`
)
