import { Nav } from './components/Nav'
import { Hero } from './components/Hero'
import { HowItWorks } from './components/HowItWorks'
import { Boundaries } from './components/Boundaries'
import { Control } from './components/Control'
import { Position } from './components/Position'
import { Install } from './components/Install'
import { Footer } from './components/Footer'

export default function App() {
  return (
    <>
      <Nav />
      <main>
        <Hero />
        <HowItWorks />
        <Boundaries />
        <Control />
        <Position />
        <Install />
      </main>
      <Footer />
    </>
  )
}
