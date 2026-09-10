#!/usr/bin/env node
// Fixture-only encoder. Loads explicitly supplied local upstream modules;
// never downloads resources. It does not implement a general glTF conversion.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');

const [modules, input, output] = process.argv.slice(2);
if (!modules || !input || !output || process.argv.length !== 5) {
  throw new Error('usage: encode-meshopt-glb.cjs UPSTREAM_JS_DIRECTORY INPUT.glb OUTPUT.glb');
}
const encoderPath = path.resolve(modules, 'meshopt_encoder.js');
const decoderPath = path.resolve(modules, 'meshopt_decoder.js');
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
assert.equal(sha(fs.readFileSync(encoderPath)), '1e5a64c0bf1fe0218e8ff80f5c800e34fa31db02cc260229c81125d88d957590', 'Use the pinned upstream 0.25 encoder');
assert.equal(sha(fs.readFileSync(decoderPath)), '05e1d7b12b8fe408b07690d701328671d026c4ace808684c942f398b1f0801d5', 'Use the pinned upstream 0.25 decoder');
const encoder = require(encoderPath);
const decoder = require(decoderPath);
const aligned = value => (value + 3) & ~3;

async function main() {
  await Promise.all([encoder.ready, decoder.ready]);
  const bytes = fs.readFileSync(input);
  assert.equal(bytes.readUInt32LE(0), 0x46546c67);
  assert.equal(bytes.readUInt32LE(4), 2);
  assert.equal(bytes.readUInt32LE(8), bytes.length);
  const jsonLength = bytes.readUInt32LE(12);
  const document = JSON.parse(bytes.subarray(20, 20 + jsonLength));
  const binary = bytes.subarray(28 + jsonLength);
  const geometryViews = new Map();
  for (const mesh of document.meshes) for (const primitive of mesh.primitives) {
    assert.equal(primitive.mode ?? 4, 4);
    assert.ok(!primitive.targets);
    // Keep geometry only. Appearance/UV/normal payloads are intentionally
    // removed from these small derivatives, with provenance recording that.
    const position = document.accessors[primitive.attributes.POSITION];
    const index = document.accessors[primitive.indices];
    assert.equal(position.componentType, 5126);
    assert.equal(position.type, 'VEC3');
    assert.ok(!position.sparse && !index.sparse);
    const stride = document.bufferViews[position.bufferView].byteStride ?? 12;
    geometryViews.set(position.bufferView, {mode: 'ATTRIBUTES', stride});
    const indexStride = {5123: 2, 5125: 4}[index.componentType];
    assert.ok(indexStride);
    geometryViews.set(index.bufferView, {mode: 'TRIANGLES', stride: indexStride});
    primitive.attributes = {POSITION: primitive.attributes.POSITION};
    delete primitive.material;
  }
  const chunks = [];
  const views = [];
  const remap = new Map();
  const references = [];
  let offset = 0;
  let fallbackOffset = 0;
  for (const [original, config] of geometryViews) {
    const oldView = document.bufferViews[original];
    assert.equal(oldView.buffer, 0);
    const source = binary.subarray(oldView.byteOffset ?? 0, (oldView.byteOffset ?? 0) + oldView.byteLength);
    assert.equal(source.length % config.stride, 0);
    const count = source.length / config.stride;
    const encoded = encoder.encodeGltfBuffer(source, count, config.stride, config.mode);
    assert.equal(encoded[0], config.mode === 'ATTRIBUTES' ? 0xa0 : 0xe1);
    const decoded = new Uint8Array(source.length);
    decoder.decodeGltfBuffer(decoded, count, config.stride, encoded, config.mode, 'NONE');
    if (config.mode === 'ATTRIBUTES') {
      assert.deepEqual(Buffer.from(decoded), source);
    } else {
      // Triangle encoding may cyclically rotate each triangle, never reverse
      // winding or change the triangle's vertex identities.
      const read = (data, at) => config.stride === 2 ? data.readUInt16LE(at) : data.readUInt32LE(at);
      const restored = Buffer.from(decoded);
      for (let i = 0; i < count; i += 3) {
        const a = [0, 1, 2].map(j => read(source, (i + j) * config.stride));
        const b = [0, 1, 2].map(j => read(restored, (i + j) * config.stride));
        assert.ok([0, 1, 2].some(k => a.every((value, j) => value === b[(j + k) % 3])));
      }
    }
    const view = {buffer: 1, byteOffset: fallbackOffset, byteLength: source.length,
      ...(config.mode === 'ATTRIBUTES' ? {byteStride: config.stride, target: 34962} : {target: 34963}),
      extensions: {EXT_meshopt_compression: {buffer: 0, byteOffset: offset,
        byteLength: encoded.length, byteStride: config.stride, count, mode: config.mode, filter: 'NONE'}}};
    remap.set(original, views.length);
    views.push(view);
    references.push({view: views.length - 1, mode: config.mode, count, stride: config.stride,
      input_sha256: sha(source), decoded_sha256: sha(decoded), compressed_sha256: sha(encoded)});
    chunks.push(Buffer.from(encoded), Buffer.alloc(aligned(encoded.length) - encoded.length));
    offset += aligned(encoded.length);
    fallbackOffset += aligned(source.length);
  }
  const accessorRemap = new Map();
  const accessors = [];
  for (const mesh of document.meshes) for (const primitive of mesh.primitives) {
    for (const [target, key] of [[primitive.attributes, 'POSITION'], [primitive, 'indices']]) {
      const original = target[key];
      if (!accessorRemap.has(original)) {
        const accessor = {...document.accessors[original]};
        accessor.bufferView = remap.get(accessor.bufferView);
        accessorRemap.set(original, accessors.length);
        accessors.push(accessor);
      }
      target[key] = accessorRemap.get(original);
    }
  }
  document.accessors = accessors;
  document.bufferViews = views;
  document.buffers = [{byteLength: offset}, {byteLength: fallbackOffset,
    extensions: {EXT_meshopt_compression: {fallback: true}}}];
  document.extensionsUsed = ['EXT_meshopt_compression'];
  document.extensionsRequired = ['EXT_meshopt_compression'];
  for (const key of ['images', 'textures', 'samplers', 'materials', 'animations', 'extensions']) delete document[key];
  document.asset.generator = 'GitTurtle fixture derivative; meshoptimizer 0.25 JavaScript encoder';
  const json = Buffer.from(JSON.stringify(document));
  const jsonChunk = Buffer.alloc(aligned(json.length), 0x20); json.copy(jsonChunk);
  const bin = Buffer.concat(chunks);
  const header = Buffer.alloc(20);
  header.writeUInt32LE(0x46546c67, 0); header.writeUInt32LE(2, 4);
  header.writeUInt32LE(28 + jsonChunk.length + bin.length, 8);
  header.writeUInt32LE(jsonChunk.length, 12); header.writeUInt32LE(0x4e4f534a, 16);
  const binHeader = Buffer.alloc(8);
  binHeader.writeUInt32LE(bin.length, 0); binHeader.writeUInt32LE(0x004e4942, 4);
  const result = Buffer.concat([header, jsonChunk, binHeader, bin]);
  fs.writeFileSync(output, result);
  console.log(JSON.stringify({source_sha256: sha(bytes), output_sha256: sha(result), bytes: result.length,
    encoder_sha256: sha(fs.readFileSync(encoderPath)), decoder_sha256: sha(fs.readFileSync(decoderPath)),
    transformation: 'Lossless POSITION and triangle indices; source transforms/scenes retained; appearance payloads omitted; required absent fallback buffer',
    views: references}, null, 2));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
