import init, {} from '../../pkg/linera_explorer'
import { mount } from '@vue/test-utils'
import Json from './Json.vue'

test('Json mounting', () => {
  init().then(() => {
    mount(Json, {
      props: { data: { foo: 42, bar: 'foo' } }
    })
  })
})
