#!/usr/bin/env node
// Independent fixture reference: upstream WASM decompression plus a deliberately
// small JavaScript static-scene/accessor interpreter. No resource resolution.
// This is validation tooling for known fixtures, not the production validator.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const [modules, output, ...inputs] = process.argv.slice(2);
assert.ok(modules && output && inputs.length, 'usage: reference-glb-geometry.cjs UPSTREAM_JS_DIRECTORY OUTPUT_DIRECTORY MODEL...');
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const modulePath = path.resolve(modules, 'meshopt_decoder.js');
assert.equal(sha(fs.readFileSync(modulePath)), '05e1d7b12b8fe408b07690d701328671d026c4ace808684c942f398b1f0801d5');
const decoder = require(modulePath);
const identity = () => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
const multiply = (a, b) => Array.from({length: 16}, (_, i) =>
  [0, 1, 2, 3].reduce((sum, k) => sum + a[k * 4 + i % 4] * b[Math.floor(i / 4) * 4 + k], 0));
function transform(node) {
  if (node.matrix) return node.matrix;
  const [x, y, z, w] = node.rotation ?? [0, 0, 0, 1];
  const [sx, sy, sz] = node.scale ?? [1, 1, 1];
  const [tx, ty, tz] = node.translation ?? [0, 0, 0];
  return [(1 - 2 * (y*y + z*z))*sx, 2*(x*y + z*w)*sx, 2*(x*z - y*w)*sx, 0,
    2*(x*y - z*w)*sy, (1 - 2*(x*x + z*z))*sy, 2*(y*z + x*w)*sy, 0,
    2*(x*z + y*w)*sz, 2*(y*z - x*w)*sz, (1 - 2*(x*x + y*y))*sz, 0, tx, ty, tz, 1];
}
function reference(bytes) {
  assert.ok(bytes.length <= 32 * 1024 * 1024);
  assert.equal(bytes.readUInt32LE(0), 0x46546c67);
  assert.equal(bytes.readUInt32LE(4), 2);
  assert.equal(bytes.readUInt32LE(8), bytes.length);
  const jsonLength = bytes.readUInt32LE(12);
  const doc = JSON.parse(bytes.subarray(20, 20 + jsonLength));
  const bin = bytes.subarray(28 + jsonLength);
  const viewCache = new Map();
  const decodedViews = [];
  function view(index) {
    if (viewCache.has(index)) return viewCache.get(index);
    const value = doc.bufferViews[index]; assert.ok(value);
    const compressed = value.extensions?.EXT_meshopt_compression;
    let result;
    if (compressed) {
      assert.equal(compressed.buffer, 0);
      assert.ok(compressed.count * compressed.byteStride <= 16 * 1024 * 1024);
      const source = bin.subarray(compressed.byteOffset ?? 0, (compressed.byteOffset ?? 0) + compressed.byteLength);
      result = Buffer.alloc(compressed.count * compressed.byteStride);
      decoder.decodeGltfBuffer(result, compressed.count, compressed.byteStride, source, compressed.mode, compressed.filter ?? 'NONE');
      decodedViews.push({view:index,mode:compressed.mode,filter:compressed.filter ?? 'NONE',bytes:result.length,sha256:sha(result)});
    } else {
      assert.equal(value.buffer, 0); assert.ok(!doc.buffers[0].uri);
      result = bin.subarray(value.byteOffset ?? 0, (value.byteOffset ?? 0) + value.byteLength);
    }
    viewCache.set(index, result);
    return result;
  }
  const types = {5120: [1,'readInt8',127],5121:[1,'readUInt8',255],5122:[2,'readInt16LE',32767],
    5123:[2,'readUInt16LE',65535],5125:[4,'readUInt32LE',4294967295],5126:[4,'readFloatLE',1]};
  function accessor(index) {
    const a = doc.accessors[index]; assert.ok(a && a.count <= 300000);
    const [size, method, divisor] = types[a.componentType];
    const width = {SCALAR:1,VEC2:2,VEC3:3,VEC4:4}[a.type]; assert.ok(width);
    const values = Array.from({length:a.count}, () => Array(width).fill(0));
    function read(bytes, offset) { return bytes[method](offset); }
    if (a.bufferView !== undefined) {
      const v = doc.bufferViews[a.bufferView]; const data = view(a.bufferView);
      const stride = v.byteStride ?? width * size;
      for (let i=0;i<a.count;i++) for (let j=0;j<width;j++) values[i][j] = read(data,(a.byteOffset ?? 0)+i*stride+j*size);
    }
    if (a.sparse) {
      const sparse = a.sparse;
      const [indexSize,indexMethod] = types[sparse.indices.componentType];
      const indices = view(sparse.indices.bufferView); const data = view(sparse.values.bufferView);
      for(let i=0;i<sparse.count;i++) {
        const at=indices[indexMethod]((sparse.indices.byteOffset ?? 0)+i*indexSize); assert.ok(at<a.count);
        for(let j=0;j<width;j++) values[at][j]=read(data,(sparse.values.byteOffset ?? 0)+(i*width+j)*size);
      }
    }
    return a.normalized ? values.map(value=>value.map(v=>Math.max(-1,v/divisor))) : values;
  }
  const triangles=[]; const minimum=[Infinity,Infinity,Infinity];const maximum=[-Infinity,-Infinity,-Infinity];
  let visits=0;
  function visit(index,parent,ancestors) {
    assert.ok(!ancestors.has(index) && ++visits <=4096); const chain=new Set(ancestors).add(index);
    const node=doc.nodes[index];assert.ok(node);assert.equal(node.skin,undefined);
    const world=multiply(parent,transform(node));
    if(node.mesh!==undefined) for(const primitive of doc.meshes[node.mesh].primitives) {
      assert.equal(primitive.mode ?? 4,4);assert.ok(!primitive.targets);
      const points=accessor(primitive.attributes.POSITION);
      const indices=primitive.indices===undefined ? points.map((_,i)=>i) : accessor(primitive.indices).flat();
      assert.equal(indices.length%3,0);
      for(let i=0;i<indices.length;i+=3) {
        assert.ok(triangles.length<100000);
        const triangle=indices.slice(i,i+3).map(at=>{
          assert.ok(at<points.length); const p=points[at];
          const placed=[0,1,2].map(axis=>world[axis]*p[0]+world[4+axis]*p[1]+world[8+axis]*p[2]+world[12+axis]);
          const point=[placed[0]*1000,-placed[2]*1000,placed[1]*1000];
          point.forEach((value,axis)=>{assert.ok(Number.isFinite(value)); minimum[axis]=Math.min(minimum[axis],value);maximum[axis]=Math.max(maximum[axis],value);});
          return point;
        });
        triangles.push(triangle);
      }
    }
    for(const child of node.children ?? []) visit(child,world,chain);
  }
  const scene=doc.scenes[doc.scene ?? 0];assert.ok(scene);
  // Decode all embedded compressed streams for reference hashes, including
  // authored normal/animation streams that static geometry does not consume.
  for(let i=0;i<(doc.bufferViews ?? []).length;i++) {
    if(doc.bufferViews[i].extensions?.EXT_meshopt_compression) view(i);
  }
  for(const node of scene.nodes ?? []) visit(node,identity(),new Set());
  assert.ok(triangles.length);
  const encoded=Buffer.alloc(triangles.length*72); let cursor=0;
  for(const triangle of triangles)for(const point of triangle)for(const coordinate of point){encoded.writeDoubleLE(coordinate,cursor);cursor+=8;}
  return {triangles:triangles.length,minimum,maximum,decoded_views:decodedViews,encoded};
}
(async()=>{
  await decoder.ready;fs.mkdirSync(output,{recursive:true});
  for(let i=0;i<inputs.length;i++) {
    const bytes=fs.readFileSync(inputs[i]);const row={name:path.basename(inputs[i]),sha256:sha(bytes)};
    try {
      const {encoded,...result}=reference(bytes);Object.assign(row,result,{ok:true,triangle_sha256:sha(encoded)});
      row.triangle_file=String(i).padStart(4,'0')+'.triangles.bin';fs.writeFileSync(path.join(output,row.triangle_file),encoded);
    } catch(error) {row.ok=false;row.error=String(error);}
    console.log(JSON.stringify(row));
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
