import init, {} from '../../pkg/linera_explorer'
import { mount } from '@vue/test-utils'
import App from './App.vue'

test('App mounting', () => {
  init().then(() => {
    mount(App)
  })
})
