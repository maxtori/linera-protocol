import init, {} from '../../pkg/linera_explorer'
import { mount } from '@vue/test-utils'
import Operations from './Operations.vue'

test('Operations mounting', () => {
  init().then(() => {
    mount(Operations, {
      props: {
        operations: [
          {
            key: {
              chain_id: "e476187f6ddfeb9d588c7b45d3df334d5501d6499b3f9ad5595cae86cce16a65",
              height: 5,
              index: 0
            },
            previousOperation: {
              chain_id: "e476187f6ddfeb9d588c7b45d3df334d5501d6499b3f9ad5595cae86cce16a65",
              height: 4, index: 0
            },
            index: 4,
            block: "f1c748c5e39591125250e85d57fdeac0b7ba44a32c12c616eb4537f93b6e5d0a",
            content: {
              System: {
                PublishBytecode: {
                  contract: { bytes: "0061..7874" },
                  service: { bytes: "0061..7874" }
                }
              }
            }
          }
        ] }
    })
  })
})
