import { composeUp, composeDown } from 'perry/compose';

const stack = await composeUp({
  version: '3.8',
  services: {
    app: {
      build: {
        context: '.',
        dockerfile: 'Dockerfile',
        args: {
          BUILD_ENV: 'production',
        },
      },
      ports: ['8080:8080'],
      environment: {
        NODE_ENV: 'production',
      },
    },
  },
});

// Tear down when done
await composeDown(stack);
