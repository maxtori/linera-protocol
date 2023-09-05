import init, {} from '../../pkg/linera_explorer'
import { mount } from '@vue/test-utils'
import InputType from './InputType.vue'

test('InputType mounting', () => {
  init().then(() => {
    mount(InputType, {
      props: {
        elt: {
          kind: 'SCALAR',
          name: 'AccountOwner'
        }, offset: false
      }
    })
  })
})
