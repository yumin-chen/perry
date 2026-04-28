/**
 * perry/compose — minimal single-service smoke test
 *
 * Brings up an nginx container, lists it, then tears it down. The
 * shortest possible end-to-end exercise of the perry/compose API.
 *
 * Run:
 *   perry main.ts -o simple
 *   ./simple                                    # uses platform default
 *   PERRY_CONTAINER_BACKEND=docker ./simple     # pin a specific runtime
 */

import { up, down, ps } from 'perry/compose';
import { getBackend } from 'perry/container';

async function main() {
  console.log(`backend: ${getBackend()}`);

  const stack = await up({
    services: {
      web: {
        image: 'nginx:alpine',
        container_name: 'perry-example-simple-nginx',
        ports: ['18080:80'],
        labels: { app: 'simple-nginx' },
      },
    },
  });
  console.log(`stack handle: ${String(stack)}`);

  // ps returns a JSON-encoded ContainerInfo[] — parse it.
  const statuses = JSON.parse(await ps(stack));
  console.log('container status:');
  for (const s of statuses) {
    console.log(`  ${s.name}\t${s.status}`);
  }

  await down(stack, { volumes: false });
  console.log('done');
}

main().catch((err) => {
  console.error('FAIL:', err);
  process.exit(1);
});
