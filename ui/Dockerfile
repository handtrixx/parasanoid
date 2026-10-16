# syntax=docker/dockerfile:1

FROM node:26-alpine AS deps
WORKDIR /app
# pnpm is used (see .gitignore). Enable corepack.
RUN corepack enable pnpm && corepack prepare pnpm@latest --activate
COPY package.json package-lock.json pnpm-lock.yaml* ./
# Generate a lockfile if missing, then install.
RUN pnpm install --frozen-lockfile || pnpm install
COPY . .

FROM deps AS build
ENV NEXT_TELEMETRY_DISABLED=1
RUN pnpm run build

FROM node:26-alpine AS production
WORKDIR /app
ENV NODE_ENV=production
RUN adduser -U 1001 -H -h /app nonroot \
    && addgroup -S nonroot
COPY --from=build /app/.next/standalone/ ./
COPY --from=build /app/.next/static ./.next/static
EXPOSE 3000
ENV PORT=3000
ENV NEXT_PUBLIC_API_BASE=http://parasanoid-server:8080
USER 1001
CMD ["node", "server.js"]
