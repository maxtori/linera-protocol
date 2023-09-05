import init, {} from '../../pkg/linera_explorer'
import { mount } from '@vue/test-utils'
import Plugin from './Plugin.vue'

test('Plugin mounting', () => {
  init().then(() => {
    mount(Plugin, {
      props: {
        plugin: {
          name: "operations",
          link: "http://localhost:8081/operations",
          queries: [],
        }
      }
    })
  })
})
