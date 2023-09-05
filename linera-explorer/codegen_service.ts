import type { CodegenConfig } from '@graphql-codegen/cli'

const config: CodegenConfig = {
  overwrite: true,
  schema: "../linera-graphql-client/graphql/service_schema.graphql",
  documents: "../linera-graphql-client/graphql/service_requests.graphql",
  generates: {
    "gql/service.d.ts": {
      plugins: ['typescript']
    },
  }
}
export default config
