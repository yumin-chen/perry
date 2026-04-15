import { composeUp, composeDown, composePs } from 'perry/compose';

const stack = await composeUp({
  version: '3.8',
  services: {
    web: {
      image: 'nginx:alpine',
      containerName: 'simple-nginx',
      ports: ['8080:80'],
      labels: {
        app: 'simple-nginx',
      },
    },
  },
});

const statuses = await composePs(stack);
console.table(statuses);

// Tear down when done
await composeDown(stack);
