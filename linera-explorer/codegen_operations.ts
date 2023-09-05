import type { CodegenConfig } from '@graphql-codegen/cli'

const config: CodegenConfig = {
  overwrite: true,
  schema: "../linera-graphql-client/graphql/operations_schema.graphql",
  documents: "../linera-graphql-client/graphql/operations_requests.graphql",
  generates: {
    "gql/operations.d.ts": {
      plugins: ['typescript']
    },
  }
}
export default config
