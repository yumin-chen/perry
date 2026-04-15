import { composeUp, composeDown, composeLogs } from 'perry/compose';

const stack = await composeUp({
  version: '3.8',
  services: {
    db: {
      image: 'postgres:16-alpine',
      environment: {
        // ${VAR:-default} interpolation is supported in string values
        POSTGRES_USER: '${DB_USER:-myuser}',
        POSTGRES_PASSWORD: '${DB_PASSWORD:-secret}',
        POSTGRES_DB: 'mydb',
      },
      volumes: ['db-data:/var/lib/postgresql/data'],
      ports: ['5432:5432'],
    },
    web: {
      image: 'myapp:latest',
      dependsOn: ['db'],
      ports: ['3000:3000'],
      environment: {
        DATABASE_URL: 'postgres://${DB_USER:-myuser}:${DB_PASSWORD:-secret}@db:5432/mydb',
      },
    },
  },
  volumes: {
    'db-data': {},
  },
});

// Stream logs from both services
const logs = await composeLogs(stack, { services: ['web', 'db'], follow: false });
console.log(logs);

// Tear down, removing named volumes
await composeDown(stack, { volumes: true });
