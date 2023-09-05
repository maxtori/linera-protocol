import init, { start, short_crypto_hash, short_app_id } from "../pkg/linera_explorer"
import { createApp, ComponentPublicInstance } from 'vue'
import { Scalars } from '../gql/operations'

import App from './components/App.vue'
import Json from "./components/Json.vue"
import Op from './components/Op.vue'
import Operation from './components/Operation.vue'
import Block from './components/Block.vue'
import Blocks from './components/Blocks.vue'
import Operations from './components/Operations.vue'
import Applications from './components/Applications.vue'
import Application from './components/Application.vue'
import Plugin from './components/Plugin.vue'
import Entrypoint from './components/Entrypoint.vue'
import InputType from './components/InputType.vue'
import OutputType from './components/OutputType.vue'

import JSONFormatter from 'json-formatter-js'

import 'bootstrap/dist/css/bootstrap.min.css'
import 'bootstrap-icons/font/bootstrap-icons.css'
import 'bootstrap/dist/js/bootstrap.bundle.min.js'

declare module '@vue/runtime-core' {
  interface ComponentCustomProperties {
    shapp: (id: string) => string,
    sh: (hash: string) => string,
    json_load: (id: string, data: any) => void,
    operation_id: (key: Scalars['OperationKey']['output']) => string,
    $root: ComponentPublicInstance<{ route: (name?: string, args?: [string, string][]) => void }>,
    }
  }

function json_load(id: string, data: any) {
  let formatter = new JSONFormatter(data, Infinity)
  let elt = document.getElementById(id)
  elt!.appendChild(formatter.render())
}

function operation_id(key: Scalars['OperationKey']['output']): string {
  return (short_crypto_hash(key.chain_id) + '-' + key.height + '-' + key.index)
}

init().then(() => {
  const app = createApp(App)
  app
    .component('v-json', Json)
    .component('v-op', Op)
    .component('v-block', Block)
    .component('v-blocks', Blocks)
    .component('v-operations', Operations)
    .component('v-operation', Operation)
    .component('v-applications', Applications)
    .component('v-application', Application)
    .component('v-plugin', Plugin)
    .component('v-entrypoint', Entrypoint)
    .component('v-input-type', InputType)
    .component('v-output-type', OutputType)
  app.config.globalProperties.sh = short_crypto_hash
  app.config.globalProperties.shapp = short_app_id
  app.config.globalProperties.json_load = json_load
  app.config.globalProperties.operation_id = operation_id
  start(app.mount('#app'))
}).catch(console.error)
