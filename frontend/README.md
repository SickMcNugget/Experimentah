```bash
curl -fsSL https://get.pnpm.io/install.sh | sh -
pnpm env use --global lts

# from this directory
pnpm i

pnpm run dev

# build release version
pnpm run build
node ./build/index.js
```

notes:

- infrastructure and experiment configs are just sent across but aren't saved to the controller, just use POST. But to keep them persistent we either have to be storing them on the filesystem (where the backend is running) after user uploads or use database. Using a database would probably not be required here as we are just dealing with simple files and it's easier to manage.