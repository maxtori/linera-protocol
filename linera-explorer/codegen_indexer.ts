import type { CodegenConfig } from '@graphql-codegen/cli'

const config: CodegenConfig = {
  overwrite: true,
  schema: "../linera-graphql-client/graphql/indexer_schema.graphql",
  documents: "../linera-graphql-client/graphql/indexer_requests.graphql",
  generates: {
    "gql/indexer.d.ts": {
      plugins: ['typescript']
    },
  }
}

export default config;
